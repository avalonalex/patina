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
use std::rc::{Rc, Weak};

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
    /// Shared behind `Rc` so cloned ports stay in sync, like `data`, and
    /// shared by every standard input port (see [`Port::stdin`]).
    pushback: Rc<RefCell<Unread>>,
}

/// Text a port has read from its source and not yet handed out.
///
/// Consumed from the front by moving an offset, so taking a character or a
/// datum off a long buffer does not copy what is left, and versioned, so a
/// reader sharing the buffer can tell whether another has taken or put back
/// text since it last looked.
#[derive(Debug, Default)]
struct Unread {
    text: String,
    /// Bytes at the front of `text` already consumed.
    start: usize,
    /// Changes whenever the unread text does.
    version: u64,
}

impl Unread {
    fn as_str(&self) -> &str {
        &self.text[self.start..]
    }

    fn consume(&mut self, bytes: usize) {
        self.start += bytes;
        debug_assert!(self.text.is_char_boundary(self.start));
        // Reclaim the consumed front once it is most of the buffer, so the
        // copy is paid for by what was consumed rather than on every call.
        if self.start == self.text.len() {
            self.text.clear();
            self.start = 0;
        } else if self.start > 4096 && self.start * 2 > self.text.len() {
            self.text.drain(..self.start);
            self.start = 0;
        }
        self.version += 1;
    }

    fn take(&mut self) -> String {
        let mut text = std::mem::take(&mut self.text);
        text.drain(..self.start);
        self.start = 0;
        self.version += 1;
        text
    }

    fn replace(&mut self, text: String) {
        self.text = text;
        self.start = 0;
        self.version += 1;
    }

    fn push_str(&mut self, text: &str) {
        self.text.push_str(text);
        self.version += 1;
    }
}

thread_local! {
    /// The unread text of standard input. There is one standard input however
    /// many ports read it, so they share this: text one of them reads ahead
    /// and leaves unconsumed is still there for the next. A program read from
    /// standard input depends on it, because the reader running the program
    /// and the program's own reads take their text from the one stream.
    static STDIN_UNREAD: Rc<RefCell<Unread>> = Rc::new(RefCell::new(Unread::default()));

    /// The file output ports opened on this thread, so that what a program left
    /// in them can be written out when it ends ([`flush_open_output_files`]).
    static OUTPUT_FILES: RefCell<OutputFiles> = const {
        RefCell::new(OutputFiles {
            ports: Vec::new(),
            prune_at: OutputFiles::MIN_PRUNE_AT,
        })
    };
}

/// Every file output port opened on a thread, held weakly. A port that has
/// been dropped flushed its writer as it went.
struct OutputFiles {
    ports: Vec<Weak<RefCell<PortData>>>,
    /// How many entries to hold before dropping those of ports that are gone,
    /// so the list follows the ports alive rather than every port ever opened,
    /// at an amortised constant cost per port.
    prune_at: usize,
}

impl OutputFiles {
    const MIN_PRUNE_AT: usize = 16;

    fn note(&mut self, data: &Rc<RefCell<PortData>>) {
        if self.ports.len() >= self.prune_at {
            self.ports.retain(|port| port.strong_count() > 0);
            self.prune_at = (2 * self.ports.len()).max(Self::MIN_PRUNE_AT);
        }
        self.ports.push(Rc::downgrade(data));
    }
}

/// Write out what every file output port still open on this thread holds in
/// its buffer, for a program that is ending, and return each port that could
/// not be written, with the reason.
///
/// A program need not close its ports, and what it wrote to one it left open
/// is kept, as chibi and Gauche keep it (#343). The process ends with
/// `std::process::exit`, which runs no destructor, so a writer's buffer that
/// nothing flushes first is lost.
pub fn flush_open_output_files() -> Vec<(PathBuf, io::Error)> {
    let ports: Vec<_> = OUTPUT_FILES.with(|files| {
        files
            .borrow()
            .ports
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    });
    let mut failures = Vec::new();
    for port in ports {
        let mut data = port.borrow_mut();
        if let PortData::File(FilePortData {
            path,
            handle: FileHandle::Output(writer),
        }) = &mut *data
            && let Err(error) = writer.flush()
        {
            failures.push((path.clone(), error));
        }
    }
    failures
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

/// Decode the UTF-8 character at `position` in `bytes`, returning it with
/// its encoded length, or `None` at the end.
///
/// This is what lets the textual operations read from a *binary* port. R7RS
/// calls that an error (i.e. leaves it open), and both references allow it —
/// chibi's ports are byte-based underneath, and Gauche accepts it too — with
/// real code relying on the freedom: chibi-mime parses the header section of
/// a binary message port with `read-line` before switching to `read-u8` for
/// the body. Invalid UTF-8 is an error, exactly as on the file-port path.
fn decode_utf8_at(bytes: &[u8], position: usize) -> io::Result<Option<(char, usize)>> {
    if position >= bytes.len() {
        return Ok(None);
    }
    let end = (position + utf8_char_len(bytes[position])).min(bytes.len());
    let s = std::str::from_utf8(&bytes[position..end])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(s.chars().next().map(|c| (c, c.len_utf8())))
}

/// The text at the front of `bytes`: its longest prefix that is valid UTF-8.
///
/// What `read` parses when its port is a binary one. A binary port need not
/// hold text all the way down — a textual header, then a body of arbitrary
/// bytes — and `read` takes one datum off the front, so it must not fail on
/// bytes it was never going to reach. The caller tells a datum that ran into
/// the undecodable part from one that ended before it.
pub fn utf8_prefix(bytes: &[u8]) -> &str {
    match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => std::str::from_utf8(&bytes[..e.valid_up_to()])
            .expect("valid_up_to bounds a valid prefix"),
    }
}

/// The encoded length a UTF-8 lead byte announces. Invalid lead bytes
/// (stray continuations, ≥ 0xF8) are deliberately lumped into 4 and left
/// for `from_utf8` to reject — every caller validates the bytes it gathers.
/// The one home for this table; it used to be written out at each read site.
fn utf8_char_len(first: u8) -> usize {
    if first & 0x80 == 0 {
        1
    } else if first & 0xE0 == 0xC0 {
        2
    } else if first & 0xF0 == 0xE0 {
        3
    } else {
        4
    }
}

/// A file port's reader, which can show the whole of the next character
/// without consuming any of it, wherever the chunks under it end.
///
/// `peek-char` has to look at a whole character without consuming any of it,
/// and a `BufRead` cannot always show one: `fill_buf` refills only an empty
/// buffer, so when a chunk ends inside a character the last thing in it is
/// that character's first bytes, and nothing short of consuming them brings
/// the rest. An ordinary text file over 8 KiB whose 8192nd byte falls inside a
/// character was enough to make `peek-char` fail (#410).
///
/// So the straddling character is assembled *here*, in `carry`, by
/// [`fill_char`](Self::fill_char): its buffered bytes are taken off the inner
/// reader, the rest follow, and `carry` is served ahead of the inner reader by
/// every method. That is what makes it sound. The bytes have moved, not gone,
/// and every read — of characters or of bytes — goes through this reader and
/// meets them in order. Parking the character in the port's text pushback
/// instead would have answered `peek-char` and lost the bytes to `read-u8` and
/// `read-bytevector`, which never look there: once per boundary, silently,
/// where there had been a loud error.
///
/// Only `fill_char` assembles one, and only `peek-char` asks it to. `fill_buf`
/// hands out what is buffered and waits for nothing more, as the inner reader
/// would: `peek-u8`, `u8-ready?` and `read_until` have no use for a whole
/// character, and waiting for one is a wait for bytes. On a file that costs
/// nothing; on a pipe the bytes may not be sent until what has already arrived
/// is answered, and the byte that was asked about is already there.
///
/// It is a concrete type in [`FileHandle::Input`], not one more boxed
/// `ReadPort`, so that a file input port cannot be built without it and
/// `peek-char` need not take it on trust.
pub struct WholeCharReader {
    inner: Box<dyn ReadPort>,
    /// The one character that straddled a chunk, when there is one; at most
    /// four bytes. Empty means every call goes to `inner`.
    carry: Vec<u8>,
    /// Bytes of `carry` already consumed.
    carry_pos: usize,
}

impl WholeCharReader {
    pub fn new(inner: Box<dyn ReadPort>) -> Self {
        WholeCharReader {
            inner,
            carry: Vec::new(),
            carry_pos: 0,
        }
    }

    fn carried(&self) -> &[u8] {
        &self.carry[self.carry_pos..]
    }

    /// The buffer, which begins with the whole of the next character unless
    /// the source ends inside it. Like `fill_buf`, it consumes nothing.
    ///
    /// The check is on the *first* byte only, which is all a peek needs, and
    /// it costs a table lookup and a comparison. Anything else passes straight
    /// through, so the inner reader's buffer is still the buffer.
    pub fn fill_char(&mut self) -> io::Result<&[u8]> {
        if self.carried().is_empty() {
            // How much is buffered, and how much the first character needs.
            // Read out as numbers so the borrow ends here: the buffer itself
            // is handed back by a second `fill_buf` below, which a non-empty
            // buffer answers without touching the source.
            let (buffered, needed) = {
                let buf = self.inner.fill_buf()?;
                (buf.len(), buf.first().map_or(0, |&b| utf8_char_len(b)))
            };
            if buffered >= needed {
                return self.inner.fill_buf();
            }
        }
        // The chunk ends inside its first character. Take what is there, then
        // what completes it. If the source ends first, the carry is a
        // truncated character, and decoding it is the error it should be.
        //
        // A carry nothing has been read from is completed whenever it is
        // asked for, not only when it is begun: an error from the source
        // leaves it part-built, with its bytes already off the inner reader,
        // and the next peek must finish it rather than decode the part. One
        // that bytes have been read from is no longer a character's start,
        // and is served as it stands.
        while self.carry_pos == 0 {
            // An empty carry needs its lead byte before it can say how long
            // the character is.
            let needed = self.carry.first().map_or(1, |&b| utf8_char_len(b));
            if self.carry.len() >= needed {
                break;
            }
            let buf = match self.inner.fill_buf() {
                Ok(buf) => buf,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if buf.is_empty() {
                break;
            }
            let take = (needed - self.carry.len()).min(buf.len());
            self.carry.extend_from_slice(&buf[..take]);
            self.inner.consume(take);
        }
        Ok(self.carried())
    }
}

impl Read for WholeCharReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.carried().is_empty() {
            return self.inner.read(buf);
        }
        // A short read, which `Read` allows: what is carried comes first, and
        // the caller comes back for the rest.
        let n = self.carried().len().min(buf.len());
        buf[..n].copy_from_slice(&self.carried()[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for WholeCharReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.carried().is_empty() {
            return self.inner.fill_buf();
        }
        Ok(self.carried())
    }

    fn consume(&mut self, amt: usize) {
        if self.carried().is_empty() {
            self.inner.consume(amt);
            return;
        }
        self.carry_pos = (self.carry_pos + amt).min(self.carry.len());
        if self.carry_pos == self.carry.len() {
            self.carry.clear();
            self.carry_pos = 0;
        }
    }
}

/// Fill `target` from `reader`, stopping short only where the source ends.
///
/// `Read::read` may return fewer bytes than it was given room for without
/// being at the end, and a buffered reader does: a request smaller than its
/// capacity is answered from what is left of the current chunk. Taking that
/// one count as final is what made `read-bytevector` return a short
/// bytevector in the *middle* of a file, where R7RS §6.13.2 lets a short
/// result mean only that the file ended (#414).
fn read_until_full_or_eof(reader: &mut dyn Read, target: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < target.len() {
        match reader.read(&mut target[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Data for a file-based port
pub struct FilePortData {
    /// The file path (for display/debugging)
    pub path: PathBuf,
    /// The file handle - either a buffered reader or writer
    pub handle: FileHandle,
}

/// File handle - either input or output.
/// Uses trait objects so the underlying stream can come from any `FileSystem` impl;
/// the input one sits inside a [`WholeCharReader`], which `peek-char` needs.
pub enum FileHandle {
    Input(WholeCharReader),
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
            pushback: Rc::new(RefCell::new(Unread::default())),
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

    /// Create a stdin port. Every stdin port shares one buffer of unread
    /// text, since they all read the one stream.
    pub fn stdin() -> Rc<Port> {
        Rc::new(Port {
            kind: PortKind::Textual,
            direction: PortDirection::Input,
            data: Rc::new(RefCell::new(PortData::Stdio(StdioKind::Stdin))),
            pushback: STDIN_UNREAD.with(Rc::clone),
        })
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
                handle: FileHandle::Input(WholeCharReader::new(reader)),
            }),
        ))
    }

    /// Open a file for writing (creates or truncates) via the given filesystem.
    pub fn open_output_file(path: &str, fs: &dyn FileSystem) -> io::Result<Rc<Port>> {
        let writer = fs.open_write(std::path::Path::new(path))?;
        Ok(Self::new_output_file(PortKind::Textual, path, writer))
    }

    /// A file output port, noted for [`flush_open_output_files`].
    fn new_output_file(kind: PortKind, path: &str, writer: Box<dyn WritePort>) -> Rc<Port> {
        let port = Self::new_port(
            kind,
            PortDirection::Output,
            PortData::File(FilePortData {
                path: PathBuf::from(path),
                handle: FileHandle::Output(writer),
            }),
        );
        OUTPUT_FILES.with(|files| files.borrow_mut().note(&port.data));
        port
    }

    /// Open a binary file for reading via the given filesystem.
    pub fn open_binary_input_file(path: &str, fs: &dyn FileSystem) -> io::Result<Rc<Port>> {
        let reader = fs.open_read(std::path::Path::new(path))?;
        Ok(Self::new_port(
            PortKind::Binary,
            PortDirection::Input,
            PortData::File(FilePortData {
                path: PathBuf::from(path),
                handle: FileHandle::Input(WholeCharReader::new(reader)),
            }),
        ))
    }

    /// Open a binary file for writing (creates or truncates) via the given filesystem.
    pub fn open_binary_output_file(path: &str, fs: &dyn FileSystem) -> io::Result<Rc<Port>> {
        let writer = fs.open_write(std::path::Path::new(path))?;
        Ok(Self::new_output_file(PortKind::Binary, path, writer))
    }

    /// Take the buffered pushback text, leaving the buffer empty.
    /// Used by `read` to resume from text it previously buffered.
    pub fn take_pushback(&self) -> String {
        self.pushback.borrow_mut().take()
    }

    /// Store text that was read from the underlying source but not
    /// consumed. Textual input operations deliver it before reading
    /// from the source again.
    pub fn set_pushback(&self, text: String) {
        self.pushback.borrow_mut().replace(text);
    }

    /// A copy of the text read from the source and not yet consumed.
    pub fn unread_text(&self) -> String {
        self.pushback.borrow().as_str().to_owned()
    }

    /// The length in bytes of the text read from the source and not yet
    /// consumed.
    pub fn unread_len(&self) -> usize {
        self.pushback.borrow().as_str().len()
    }

    /// Consume the first `bytes` bytes of the unread text, which must end on
    /// a character boundary.
    pub fn consume_unread(&self, bytes: usize) {
        self.pushback.borrow_mut().consume(bytes);
    }

    /// A number that changes whenever the unread text does, whichever port
    /// sharing it consumed, replaced or added to it.
    pub fn unread_version(&self) -> u64 {
        self.pushback.borrow().version
    }

    /// Read the next line from the source onto the end of the unread text and
    /// return it, or `None` at the end of the source.
    ///
    /// For a reader that has to see a line before it knows how much of the
    /// line to consume, and that leaves the rest readable through the port.
    pub fn pull_line(&self) -> io::Result<Option<String>> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }
        let line = self.read_line_from_source()?;
        if let Some(line) = &line {
            self.pushback.borrow_mut().push_str(line);
        }
        Ok(line)
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

    /// Whether this port was *opened* as a textual one.
    ///
    /// Not what `textual-port?` answers: the textual operations work on a
    /// binary port too, in both directions, so to Scheme every port is
    /// textual (see `textual_port_p`). The kind still decides what
    /// [`is_binary`](Self::is_binary) says, and so where the byte operations
    /// are refused.
    pub fn is_textual(&self) -> bool {
        self.kind == PortKind::Textual
    }

    /// Check if this is a binary port
    pub fn is_binary(&self) -> bool {
        self.kind == PortKind::Binary
    }

    /// Close the port. For file output ports, finalizes (flushes) the writer first.
    pub fn close(&self) {
        self.pushback.borrow_mut().replace(String::new());
        let mut data = self.data.borrow_mut();
        // Finalize write ports before closing
        if let PortData::File(ref mut fp) = *data
            && let FileHandle::Output(ref mut writer) = fp.handle
        {
            let _ = writer.finalize();
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
            if let Some(ch) = pb.as_str().chars().next() {
                pb.consume(ch.len_utf8());
                return Ok(Some(ch));
            }
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::String(s) => {
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
                        let char_len = utf8_char_len(buf[0]);
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
            PortData::File(fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    let mut buf = [0u8; 4]; // Max UTF-8 char size
                    match reader.read(&mut buf[..1]) {
                        Ok(0) => Ok(None), // EOF
                        Ok(_) => {
                            // Determine UTF-8 character length from first byte
                            let char_len = utf8_char_len(buf[0]);
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
            // Textual reads from a binary port decode UTF-8 in place — see
            // `decode_utf8_at` for why this is allowed at all.
            PortData::Bytevector(b) => match decode_utf8_at(&b.content, b.position)? {
                Some((ch, len)) => {
                    b.position += len;
                    Ok(Some(ch))
                }
                None => Ok(None),
            },
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
            PortData::Bytevector(b) => {
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
            PortData::File(fp) => {
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
            PortData::File(fp) => {
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
            PortData::File(fp) => {
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
            PortData::Bytevector(b) => {
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
            PortData::File(fp) => {
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
        if let Some(ch) = self.pushback.borrow().as_str().chars().next() {
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
            PortData::File(fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    // The first character only. Validating the whole chunk
                    // to return one character failed wherever a chunk ended
                    // inside a character, or held a byte further along that
                    // is not text at all (#410). `fill_char` sees to it that
                    // the first character is all there unless the file ends
                    // inside it.
                    let buf = reader.fill_char()?;
                    Ok(decode_utf8_at(buf, 0)?.map(|(ch, _)| ch))
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an input file port",
                    ))
                }
            }
            PortData::Bytevector(b) => {
                Ok(decode_utf8_at(&b.content, b.position)?.map(|(ch, _)| ch))
            }
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

        if !self.pushback.borrow().as_str().is_empty() {
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
            PortData::File(fp) => {
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
            // Mirrors the String arm above: ready iff data remains. Note the
            // whole family (String/File/u8_ready too) returns false at EOF,
            // where R7RS 6.13.2 says #t (a read would not block — it returns
            // the EOF object); a fix belongs to all arms at once, not here.
            PortData::Bytevector(b) => Ok(b.position < b.content.len()),
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
            PortData::String(port_data) => {
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
            PortData::File(fp) => {
                if let FileHandle::Output(ref mut writer) = fp.handle {
                    writer.write_all(s.as_bytes())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an output file port",
                    ))
                }
            }
            // Text goes into a binary port as the UTF-8 it is read back out
            // of one as (`decode_utf8_at`) — the same freedom, in the other
            // direction, and both references take it too. Refusing here while
            // the reads were allowed was half a decision: chibi-binary-record
            // writes its string and character fields to a bytevector port
            // with `write-string` and `write-char`, between `write-u8`s, and
            // everything built on it (chibi-tar) stopped at this arm (#404).
            // A binary *file* port never refused: the `File` arm above writes
            // bytes whatever kind the port was opened as.
            PortData::Bytevector(port_data) => {
                port_data.content.extend_from_slice(s.as_bytes());
                Ok(())
            }
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
            PortData::File(fp) => {
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
            PortData::Bytevector(b) => {
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
            PortData::File(fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    // k limits consumption, not allocation (#417). Grow with
                    // the bytes read, retrying short/interrupted reads until
                    // that limit or EOF, as read-bytevector! does (#414).
                    let mut buf = Vec::new();
                    match reader.take(k as u64).read_to_end(&mut buf)? {
                        0 => Ok(None), // EOF
                        _ => Ok(Some(buf)),
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
            PortData::Bytevector(b) => {
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
            PortData::File(fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    match read_until_full_or_eof(reader, target)? {
                        0 => Ok(None), // EOF
                        n => Ok(Some(n)),
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
            PortData::Bytevector(b) => {
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
            PortData::File(fp) => {
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
        {
            let mut pb = self.pushback.borrow_mut();
            if let Some(newline_pos) = pb.as_str().find('\n') {
                let line = pb.as_str()[..=newline_pos].to_owned();
                pb.consume(newline_pos + 1);
                return Ok(Some(line));
            }
        }
        let mut prefix = self.take_pushback();
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
            PortData::String(s) => {
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
            PortData::File(fp) => {
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
            PortData::Bytevector(b) => {
                if b.position >= b.content.len() {
                    return Ok(None);
                }
                let remaining = &b.content[b.position..];
                let end = remaining
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .map(|i| i + 1)
                    .unwrap_or(remaining.len());
                let line = std::str::from_utf8(&remaining[..end])
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
                    .to_string();
                b.position += end;
                Ok(Some(line))
            }
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

    /// Advance the position of an in-memory input port by `bytes_consumed`,
    /// after `read` has parsed a datum out of the text ahead of it. Bytes, not
    /// characters, on both kinds: a string port's position indexes its UTF-8.
    pub fn advance_position(&self, bytes_consumed: usize) -> io::Result<()> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::String(s) => {
                s.position += bytes_consumed;
                Ok(())
            }
            PortData::Bytevector(b) => {
                b.position += bytes_consumed;
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an in-memory port",
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

    /// Every standard input port reads the one stream, so text one of them
    /// read ahead and left unconsumed is there for the others.
    #[test]
    fn stdin_ports_share_their_unread_text() {
        let reader = Port::stdin();
        let program = Port::stdin();
        reader.set_pushback("(a) (b)\n".to_string());
        assert_eq!(program.read_char().unwrap(), Some('('));
        assert_eq!(reader.unread_text(), "a) (b)\n");
        assert_eq!(program.take_pushback(), "a) (b)\n");
    }

    /// A reader that pulls lines onto the unread text and consumes it from
    /// the front can tell, by its version, whether anything else took text.
    #[test]
    fn pulled_lines_are_unread_until_consumed_and_consuming_changes_the_version() {
        let port = Port::new_input_string("one\ntwo\n".to_string());
        let before = port.unread_version();
        assert_eq!(port.pull_line().unwrap().as_deref(), Some("one\n"));
        assert_eq!(port.unread_text(), "one\n");
        port.consume_unread(2);
        assert_eq!(port.unread_text(), "e\n");
        assert_ne!(port.unread_version(), before);

        let version = port.unread_version();
        assert_eq!(port.peek_char().unwrap(), Some('e'));
        assert_eq!(port.unread_version(), version, "peeking takes nothing");
        assert_eq!(port.pull_line().unwrap().as_deref(), Some("two\n"));
        assert_eq!(port.read_line().unwrap().as_deref(), Some("e\n"));
        assert_eq!(port.unread_text(), "two\n");
        assert_eq!(port.pull_line().unwrap(), None);
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
    fn test_textual_reads_on_binary_port_decode_utf8() {
        // Textual reads on a binary port decode UTF-8 — both references
        // allow this and chibi-mime relies on it; see `decode_utf8_at`.
        let port = Port::new_input_bytevector(b"AB\nrest".to_vec());
        assert_eq!(port.peek_char().unwrap(), Some('A'));
        assert_eq!(port.read_char().unwrap(), Some('A'));
        assert_eq!(port.read_line().unwrap(), Some("B\n".to_string()));
        assert_eq!(port.read_line().unwrap(), Some("rest".to_string()));
        assert_eq!(port.read_char().unwrap(), None);
        // Pins consistency with the sibling arms, not R7RS: 6.13.2 wants #t
        // at EOF, and every in-memory arm predates this one in saying false.
        // Flip this together with them if that family-wide deviation is fixed.
        assert!(!port.char_ready().unwrap());

        // A multi-byte character decodes whole, not byte-by-byte.
        let port = Port::new_input_bytevector("λx".as_bytes().to_vec());
        assert_eq!(port.read_char().unwrap(), Some('λ'));
        assert_eq!(port.read_char().unwrap(), Some('x'));

        // Invalid UTF-8 is an error, as on the file path.
        let port = Port::new_input_bytevector(vec![0xFF, 0xFE]);
        assert!(port.read_char().is_err());
    }

    #[test]
    fn test_textual_writes_on_binary_port_encode_utf8() {
        // The other half of the test above: text goes into a binary port as
        // UTF-8, interleaved with bytes, at one position. Both references
        // allow it and chibi-binary-record relies on it (#404).
        let port = Port::new_output_bytevector();
        port.write_u8(1).unwrap();
        port.write_string("aλ").unwrap();
        port.write_char('!').unwrap();
        port.write_u8(2).unwrap();
        assert_eq!(
            port.get_output_bytevector().unwrap(),
            vec![1, b'a', 0xCE, 0xBB, b'!', 2]
        );

        // Still an output port only, and still closable.
        assert!(
            Port::new_input_bytevector(vec![])
                .write_string("x")
                .is_err()
        );
        port.close();
        assert!(port.write_string("x").is_err());
    }

    #[test]
    fn test_utf8_prefix_stops_where_the_text_does() {
        assert_eq!(utf8_prefix(b"all text"), "all text");
        assert_eq!(utf8_prefix(&[b'x', b' ', 0xFF, b'y']), "x ");
        assert_eq!(utf8_prefix(&[0xFF]), "");
        assert_eq!(utf8_prefix(&[]), "");
        // A character cut off by the end of the bytes is not text either.
        assert_eq!(utf8_prefix(&[b'a', 0xCE]), "a");
    }

    #[test]
    fn test_advance_position_moves_a_bytevector_port_by_bytes() {
        // What `read` does after parsing a datum off the front: the binary
        // operations continue from the byte after it.
        let port = Port::new_input_bytevector(vec![b'x', b' ', 0xFF]);
        port.advance_position(1).unwrap();
        assert_eq!(port.read_u8().unwrap(), Some(b' '));
        assert_eq!(port.read_u8().unwrap(), Some(0xFF));
    }

    /// A `WholeCharReader` over `bytes`, whose inner reader hands out chunks
    /// of at most `capacity` — so a chunk boundary can be put anywhere.
    fn chunked(bytes: &[u8], capacity: usize) -> WholeCharReader {
        let inner = io::BufReader::with_capacity(capacity, io::Cursor::new(bytes.to_vec()));
        WholeCharReader::new(Box::new(inner))
    }

    #[test]
    fn test_whole_char_reader_never_begins_a_buffer_inside_a_character() {
        // One-, two-, three- and four-byte characters, and every chunk size
        // that can split them. At each character boundary `fill_char` must
        // begin with that whole character, whatever the chunking; consuming
        // a character at a time must visit every character, in order.
        let text = "aλ€𝄞bλλ€";
        for capacity in 1..=9 {
            let mut reader = chunked(text.as_bytes(), capacity);
            let mut seen = String::new();
            loop {
                let buf = reader.fill_char().unwrap();
                let Some((ch, len)) = decode_utf8_at(buf, 0).unwrap() else {
                    break;
                };
                seen.push(ch);
                reader.consume(len);
            }
            assert_eq!(seen, text, "chunks of {capacity}");
        }
    }

    #[test]
    fn test_whole_char_reader_hands_every_byte_to_every_kind_of_read() {
        // The carried character's bytes have moved, not gone: whichever
        // method reads next meets them first, in order. This is the property
        // that parking the character in a *text* pushback would not have.
        let bytes = "xλy".as_bytes(); // 78 CE BB 79
        for capacity in 1..=4 {
            // Byte by byte through `read`, peeking a character before each.
            let mut reader = chunked(bytes, capacity);
            let mut out = Vec::new();
            loop {
                let _ = reader.fill_char().unwrap();
                let mut one = [0u8; 1];
                if reader.read(&mut one).unwrap() == 0 {
                    break;
                }
                out.push(one[0]);
            }
            assert_eq!(out, bytes, "read, chunks of {capacity}");

            // All at once, after a peek has assembled a carry.
            let mut reader = chunked(bytes, capacity);
            assert_eq!(reader.fill_char().unwrap()[0], b'x');
            reader.consume(1);
            assert_eq!(reader.fill_char().unwrap()[..2], [0xCE, 0xBB]);
            let mut rest = [0u8; 3];
            assert_eq!(read_until_full_or_eof(&mut reader, &mut rest).unwrap(), 3);
            assert_eq!(rest, [0xCE, 0xBB, b'y'], "block read, chunks of {capacity}");

            // Through `read_until`, which is built on `fill_buf`/`consume`.
            let mut reader = chunked(bytes, capacity);
            let _ = reader.fill_char().unwrap();
            let mut line = Vec::new();
            reader.read_until(b'y', &mut line).unwrap();
            assert_eq!(line, bytes, "read_until, chunks of {capacity}");
        }
    }

    #[test]
    fn test_whole_char_reader_consumes_a_carry_a_byte_at_a_time() {
        let mut reader = chunked("λz".as_bytes(), 1);
        assert_eq!(reader.fill_char().unwrap(), [0xCE, 0xBB]);
        reader.consume(1);
        // What is left of it is no character's start, and is not topped up
        // as one: both views serve it as it stands.
        assert_eq!(reader.fill_buf().unwrap(), [0xBB]);
        assert_eq!(reader.fill_char().unwrap(), [0xBB]);
        reader.consume(1);
        assert_eq!(reader.fill_buf().unwrap(), [b'z']);
        reader.consume(1);
        assert!(reader.fill_buf().unwrap().is_empty());
    }

    #[test]
    fn test_whole_char_reader_at_a_source_that_ends_inside_a_character() {
        // Nothing can complete it, so what there is comes out — and decoding
        // it is the error a truncated file deserves, not an end of file.
        let mut reader = chunked(&[b'a', 0xCE], 1);
        assert_eq!(reader.fill_char().unwrap(), [b'a']);
        reader.consume(1);
        assert_eq!(reader.fill_char().unwrap(), [0xCE]);
        assert!(decode_utf8_at(reader.fill_char().unwrap(), 0).is_err());
        reader.consume(1);
        assert!(reader.fill_char().unwrap().is_empty());

        // A byte that leads no character at all is not waited on forever
        // either: it is topped up like a four-byte lead and then rejected.
        let mut reader = chunked(&[0xFF, b'a', b'b'], 1);
        assert_eq!(reader.fill_char().unwrap(), [0xFF, b'a', b'b']);
        assert!(decode_utf8_at(reader.fill_char().unwrap(), 0).is_err());
    }

    /// A source that answers each `read` with the next step of a script: some
    /// bytes, or an error. What a pipe looks like to its reader, where the
    /// next bytes are not there yet — which a `Cursor` can never show.
    struct Scripted(std::collections::VecDeque<io::Result<Vec<u8>>>);

    impl Read for Scripted {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.0.pop_front() {
                None => Ok(0),
                Some(Err(e)) => Err(e),
                Some(Ok(bytes)) => {
                    buf[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
            }
        }
    }

    fn scripted(steps: Vec<io::Result<Vec<u8>>>) -> WholeCharReader {
        let inner = io::BufReader::with_capacity(8, Scripted(steps.into()));
        WholeCharReader::new(Box::new(inner))
    }

    #[test]
    fn test_whole_char_reader_waits_for_a_character_only_when_asked_for_one() {
        // The first byte of a `λ` has arrived and the second has not. A byte
        // peek is answered from what is there: going back to the source would
        // be a wait, on a pipe, for bytes its writer may not send until this
        // one is answered — and here it would be the error, which
        // `fill_buf` must therefore never meet.
        let would_block = || Err(io::ErrorKind::WouldBlock.into());
        let mut reader = scripted(vec![Ok(vec![0xCE]), would_block(), Ok(vec![0xBB, b'z'])]);
        assert_eq!(reader.fill_buf().unwrap(), [0xCE]);
        assert_eq!(reader.fill_buf().unwrap(), [0xCE]);

        // A character peek does go back for the rest, and meets the error
        // with the first byte already carried. Nothing is lost by that: the
        // byte is still next, and the next character peek finishes the
        // character rather than decoding the half of it.
        let error = reader.fill_char().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(reader.fill_buf().unwrap(), [0xCE]);
        assert_eq!(reader.fill_char().unwrap(), [0xCE, 0xBB]);
        reader.consume(2);
        assert_eq!(reader.fill_char().unwrap(), [b'z']);
    }

    #[test]
    fn test_whole_char_reader_assembles_a_character_through_an_interrupted_read() {
        let interrupted = || Err(io::ErrorKind::Interrupted.into());
        let mut reader = scripted(vec![
            Ok(vec![0xE2]),
            interrupted(),
            Ok(vec![0x82]),
            interrupted(),
            Ok(vec![0xAC]),
        ]);
        let buf = reader.fill_char().unwrap();
        assert_eq!(decode_utf8_at(buf, 0).unwrap(), Some(('€', 3)));
    }

    #[test]
    fn test_read_until_full_or_eof_outlasts_short_reads() {
        // Chunks of three, a request for seven: three reads, one result.
        let mut reader = chunked(&[1, 2, 3, 4, 5, 6, 7, 8], 3);
        let mut target = [0u8; 7];
        assert_eq!(read_until_full_or_eof(&mut reader, &mut target).unwrap(), 7);
        assert_eq!(target, [1, 2, 3, 4, 5, 6, 7]);
        // Short only at the end, and zero only when nothing is left.
        let mut target = [0u8; 7];
        assert_eq!(read_until_full_or_eof(&mut reader, &mut target).unwrap(), 1);
        assert_eq!(read_until_full_or_eof(&mut reader, &mut target).unwrap(), 0);
    }

    #[test]
    fn test_file_read_bytevector_allocates_for_the_data_not_the_limit() {
        let fs = crate::vfs::MemoryFs::new();
        let contents: Vec<u8> = (0..10000).map(|i| (i % 250) as u8).collect();
        fs.add_file("/bytes.bin", contents.clone());
        let port = Port::open_binary_input_file("/bytes.bin", &fs).unwrap();

        // A modest limit keeps the old eager allocation safe to test while
        // exposing its retained capacity. The CLI test covers an abort-sized
        // limit in a child process, where it cannot kill the test runner.
        let bytes = port.read_bytevector(1 << 20).unwrap().unwrap();
        assert_eq!(bytes, contents);
        assert!(
            bytes.capacity() <= 64 * 1024,
            "10 KB of data retained {} bytes for a 1 MB limit",
            bytes.capacity()
        );
        assert_eq!(port.read_bytevector(1 << 20).unwrap(), None);
        assert_eq!(port.read_bytevector(0).unwrap(), Some(vec![]));
    }

    #[test]
    fn test_file_read_bytevector_retries_short_and_interrupted_reads() {
        let reader = scripted(vec![
            Err(io::ErrorKind::Interrupted.into()),
            Ok(vec![1, 2]),
            Err(io::ErrorKind::Interrupted.into()),
            Ok(vec![3, 4]),
            Ok(vec![5]),
            Err(io::ErrorKind::PermissionDenied.into()),
        ]);
        let port = Port::new_port(
            PortKind::Binary,
            PortDirection::Input,
            PortData::File(FilePortData {
                path: "scripted".into(),
                handle: FileHandle::Input(reader),
            }),
        );
        // Neither a zero-length request nor reaching the limit may consume
        // the next byte. Only Interrupted is retried; other errors survive.
        assert_eq!(port.read_bytevector(0).unwrap(), Some(vec![]));
        assert_eq!(port.read_bytevector(4).unwrap(), Some(vec![1, 2, 3, 4]));
        assert_eq!(port.read_u8().unwrap(), Some(5));
        assert_eq!(
            port.read_bytevector(4).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
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
