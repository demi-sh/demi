//! Bytes that are no UTF-8 in the shell's text.
//!
//! The shell's values are Rust strings, which hold UTF-8 alone, while bash
//! keeps whatever bytes it reads: a Latin-1 or GBK file through `$(cat f)`,
//! a file name, `$'\xff'`. So each byte that is no UTF-8, always one of
//! 0x80 to 0xFF, is held as a character of its own, U+10FF00 plus the byte,
//! the last 128 of the private-use plane 16, and is written as that byte
//! again wherever the shell's text leaves it: a builtin's output, a
//! program's arguments and environment, a path. The byte counts as one
//! character, as in bash.
//!
//! ```text
//!   bytes in ──decode──> "a\u{10FFFF}b" ──encode──> bytes out
//!   61 ff 62                                       61 ff 62
//! ```
//!
//! Those 128 characters stand for nothing else: one read as UTF-8 is held
//! as its four bytes, as fish does with the code points its scheme takes,
//! so every byte comes out as it went in. A text joined from parts, such as
//! `$a$b` where `a` ends with the first byte of a character `b` ends, is
//! made whole again by `normalize`, so it is the text bash's would be.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::Path;

/// The character that would hold the byte 0x00; only 0x80 to 0xFF are held.
const BASE: u32 = 0x10_FF00;

/// The character holding `byte`, one of 0x80 to 0xFF, which is no UTF-8
/// where it was read.
pub fn byte_char(byte: u8) -> char {
    debug_assert!(byte >= 0x80, "a byte below 0x80 is ASCII and never held");
    char::from_u32(BASE + u32::from(byte)).expect("plane 16 holds U+10FF80 to U+10FFFF")
}

/// The byte `c` holds, if it holds one (`byte_char`).
pub fn char_byte(c: char) -> Option<u8> {
    u32::from(c)
        .checked_sub(BASE)
        .and_then(|offset| u8::try_from(offset).ok())
        .filter(|byte| *byte >= 0x80)
}

/// Whether `text` holds a byte, which only then needs converting.
fn holds(text: &str) -> bool {
    // A held byte's character is F4 8F BE xx or F4 8F BF xx in UTF-8.
    text.as_bytes().windows(3).any(|w| w[0] == 0xF4 && w[1] == 0x8F && w[2] >= 0xBE)
}

/// `bytes` as the shell's text: UTF-8 as it is, each other byte as its
/// `byte_char`, and a character that is one of those as its bytes.
pub fn decode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        let valid = chunk.valid();
        if holds(valid) {
            for c in valid.chars() {
                if char_byte(c).is_some() {
                    text.extend(c.encode_utf8(&mut [0; 4]).bytes().map(byte_char));
                } else {
                    text.push(c);
                }
            }
        } else {
            text.push_str(valid);
        }
        text.extend(chunk.invalid().iter().map(|byte| byte_char(*byte)));
    }
    text
}

/// `bytes` as the shell's text, borrowed where they are all UTF-8.
pub fn decode_cow(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) if !holds(text) => Cow::Borrowed(text),
        _ => Cow::Owned(decode(bytes)),
    }
}

/// The bytes `text` stands for: its UTF-8, with each `byte_char` as its byte.
pub fn encode(text: &str) -> Cow<'_, [u8]> {
    if !holds(text) {
        return Cow::Borrowed(text.as_bytes());
    }
    let mut bytes = Vec::with_capacity(text.len());
    for c in text.chars() {
        match char_byte(c) {
            Some(byte) => bytes.push(byte),
            None => bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
    Cow::Owned(bytes)
}

/// `text` as `decode` would read the bytes it stands for: held bytes that
/// joining parts made a whole character become the character.
pub fn normalize(text: &str) -> Cow<'_, str> {
    if !holds(text) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(decode(&encode(text)))
}

/// Appends `suffix` to `base`, as `+=` does: held bytes where they meet may
/// make one character (`normalize`).
pub fn append(base: &mut String, suffix: &str) {
    let meet = base.chars().next_back().is_some_and(|c| char_byte(c).is_some())
        && suffix.chars().next().is_some_and(|c| char_byte(c).is_some());
    base.push_str(suffix);
    if meet && let Cow::Owned(whole) = normalize(base) {
        *base = whole;
    }
}

/// `text` for a protocol whose strings are UTF-8, such as a declared
/// command's arguments: a held byte is U+FFFD there.
pub fn lossy(text: &str) -> Cow<'_, str> {
    match encode(text) {
        Cow::Borrowed(_) => Cow::Borrowed(text),
        Cow::Owned(bytes) => Cow::Owned(String::from_utf8_lossy(&bytes).into_owned()),
    }
}

/// `raw`, the UTF-8 of the shell's text, with each `byte_char` written as
/// its byte.
fn unhold(raw: &[u8]) -> Cow<'_, [u8]> {
    match std::str::from_utf8(raw) {
        Ok(text) => encode(text),
        // The text's UTF-8 cut short, which `ByteWriter` holds back.
        Err(_) => {
            let mut bytes = Vec::with_capacity(raw.len());
            for chunk in raw.utf8_chunks() {
                bytes.extend_from_slice(&encode(chunk.valid()));
                bytes.extend_from_slice(chunk.invalid());
            }
            Cow::Owned(bytes)
        }
    }
}

/// How many bytes `text` stands for (`encode`), which a locale of one byte
/// a character counts.
pub fn byte_len(text: &str) -> usize {
    text.chars().map(|c| if char_byte(c).is_some() { 1 } else { c.len_utf8() }).sum()
}

/// `name`, an argument, a variable's value or a path from the system, as
/// the shell's text.
#[cfg(unix)]
pub fn from_os(name: &OsStr) -> Cow<'_, str> {
    use std::os::unix::ffi::OsStrExt as _;
    decode_cow(name.as_bytes())
}

/// `name`, an argument, a variable's value or a path from the system, as
/// the shell's text.
#[cfg(not(unix))]
pub fn from_os(name: &OsStr) -> Cow<'_, str> {
    name.to_string_lossy()
}

/// `text` as the system's string, for a program's argument or environment
/// or a path.
#[cfg(unix)]
pub fn to_os(text: &str) -> Cow<'_, OsStr> {
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
    match encode(text) {
        Cow::Borrowed(bytes) => Cow::Borrowed(OsStr::from_bytes(bytes)),
        Cow::Owned(bytes) => Cow::Owned(OsString::from_vec(bytes)),
    }
}

/// `text` as the system's string, for a program's argument or environment
/// or a path. Windows' strings hold no bytes, so a held byte is U+FFFD there.
#[cfg(not(unix))]
pub fn to_os(text: &str) -> Cow<'_, OsStr> {
    match lossy(text) {
        Cow::Borrowed(text) => Cow::Borrowed(OsStr::new(text)),
        Cow::Owned(text) => Cow::Owned(OsString::from(text)),
    }
}

/// `path`, made from the shell's text, as the path of what it stands for
/// (`to_os`). A path the system gave is no text and is not passed here.
pub fn path(path: &Path) -> Cow<'_, Path> {
    match path.to_str() {
        Some(text) if holds(text) => Cow::Owned(to_os(text).into_owned().into()),
        _ => Cow::Borrowed(path),
    }
}

/// A writer of the shell's text, as a builtin's output, that writes each
/// `byte_char` as its byte. A character split across two writes is joined
/// first; what a write leaves pending goes out with the next write, a
/// flush or the drop. Bytes that are no text, such as printf's, go to the
/// stream itself.
pub struct ByteWriter<W: std::io::Write> {
    inner: W,
    /// The start of a UTF-8 character the last write cut.
    pending: Vec<u8>,
}

impl<W: std::io::Write> ByteWriter<W> {
    /// Writes to `inner`.
    pub const fn new(inner: W) -> Self {
        Self { inner, pending: Vec::new() }
    }

    fn put(&mut self, buf: &[u8]) -> std::io::Result<()> {
        let joined;
        let buf = if self.pending.is_empty() {
            buf
        } else {
            self.pending.extend_from_slice(buf);
            joined = std::mem::take(&mut self.pending);
            joined.as_slice()
        };
        // The start of a held byte's character, F4 8F BE..BF xx, that the
        // write cuts goes with the next.
        let starts = |tail: &[u8]| match tail {
            [0xF4] | [0xF4, 0x8F] => true,
            [0xF4, 0x8F, third] => *third >= 0xBE,
            _ => false,
        };
        let cut = (1..=3.min(buf.len()))
            .find(|&count| starts(&buf[buf.len() - count..]))
            .unwrap_or(0);
        let (whole, rest) = buf.split_at(buf.len() - cut);
        self.inner.write_all(&unhold(whole))?;
        self.pending.extend_from_slice(rest);
        Ok(())
    }

    fn drain(&mut self) -> std::io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let pending = std::mem::take(&mut self.pending);
        self.inner.write_all(&pending)
    }
}

impl<W: std::io::Write> std::io::Write for ByteWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.put(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.drain()?;
        self.inner.flush()
    }
}

impl<W: std::io::Write> Drop for ByteWriter<W> {
    fn drop(&mut self) {
        let _ = self.drain();
    }
}
