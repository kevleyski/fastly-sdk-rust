//! HTTP bodies.

pub(crate) mod handle;
pub(crate) mod streaming;

use self::handle::BodyHandle;
use std::fmt::Debug;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::mem::{self, ManuallyDrop};

pub use streaming::StreamingBody;

/// An HTTP body that can be read from, written to, or appended to another body.
///
/// The most efficient ways to read from and write to the body are through the [`Read`],
/// [`BufRead`], and [`Write`] implementations.
///
/// Read and write operations to a [`Body`] are automatically buffered, though you can take direct
/// control over aspects of the buffering using the [`BufRead`] methods and [`Write::flush()`].
pub struct Body {
    // NOTE: The order of these fields with these different handles is load
    // bearing. `BufWriter` needs `BodyHandle` so that it flushes out the buffer
    // and then drops the `BodyHandle` out properly when `Body` is dropped.
    // `BodyHandleWrapper` makes sure we don't double free the memory that
    // `BodyHandle` points too.
    reader: BufReader<BodyHandleWrapper>,
    writer: BufWriter<BodyHandle>,
}

impl Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<opaque Body>")
    }
}

impl Body {
    /// Get a new, empty HTTP body.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        BodyHandle::new().into()
    }

    // this is not exported, since misuse can lead to data getting dropped or appearing out of order
    fn handle(&mut self) -> &mut BodyHandle {
        self.writer.get_mut()
    }

    /// Convert a [`Body`] into the low-level [`BodyHandle`] interface.
    pub fn into_handle(mut self) -> BodyHandle {
        self.put_back_read_buf();
        // Flushes the buffer and returns the underlying `BodyHandle`
        self.writer
            .into_inner()
            .expect("fastly_http_body::write failed")
    }

    /// Put any currently buffered read data back at the front of the body.
    fn put_back_read_buf(&mut self) {
        let read_buf = self.reader.buffer();
        if !read_buf.is_empty() {
            // We have to cheat a little here to get mutable access to the handle while the reader
            // buffer is borrowed. Since we're not going to read or write through the `self`
            // interface while `body_handle` is live, no other aliases of the handle will be used.
            let mut body_handle =
                ManuallyDrop::new(unsafe { BodyHandle::from_u32(self.writer.get_ref().as_u32()) });
            let nwritten = body_handle.write_front(read_buf);
            drop(read_buf);
            // Let the `BufReader` know that we've consumed these bytes from its internal buffer so
            // it won't yield them for a subsequent read.
            self.reader.consume(nwritten)
        };
    }

    /// Read the entirety of the body into a byte vector.
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body.md")]
    pub fn into_bytes(self) -> Vec<u8> {
        self.into_handle().into_bytes()
    }

    /// Read the entirety of the body into a `String`, interpreting the bytes as UTF-8.
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body.md")]
    ///
    /// # Panics
    ///
    #[doc = include_str!("../../docs/snippets/panics-body-utf8.md")]
    pub fn into_string(self) -> String {
        self.into_handle().into_string()
    }

    /// Append another body onto the end of this body.
    ///
    #[doc = include_str!("../../docs/snippets/body-append-constant-time.md")]
    pub fn append(&mut self, other: Body) {
        // flush the write buffer of the destination body, so that we can use the append method on
        // the underlying handles. Unwrap, as `BodyHandle::flush` won't return actionable errors.
        self.writer.flush().expect("fastly_http_body::write failed");
        self.handle().append(other.into_handle())
    }

    /// Write a slice of bytes to the end of this body, and return the number of bytes written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # let mut body = fastly::Body::new();
    /// body.write_bytes(&[0, 1, 2, 3]);
    /// ```
    pub fn write_bytes(&mut self, bytes: &[u8]) -> usize {
        self.writer
            .write(bytes)
            .expect("fastly_http_body::write failed")
    }

    /// Write a string slice to the end of this body, and return the number of bytes written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # let mut body = fastly::Body::new();
    /// body.write_str("woof woof");
    /// ```
    pub fn write_str(&mut self, string: &str) -> usize {
        self.write_bytes(string.as_ref())
    }

    /// Return an iterator that reads the body in chunks of at most the given number of bytes.
    ///
    /// If `chunk_size` does not evenly divide the length of the body, then the last chunk will not
    /// have length `chunk_size`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Body;
    /// fn remove_0s(body: &mut Body) {
    ///     let mut no_0s = Body::new();
    ///     for chunk in body.read_chunks(4096) {
    ///         let mut chunk = chunk.unwrap();
    ///         chunk.retain(|b| *b != 0);
    ///         no_0s.write_bytes(&chunk);
    ///     }
    ///     *body = no_0s;
    /// }
    /// ```
    pub fn read_chunks<'a>(
        &'a mut self,
        chunk_size: usize,
    ) -> impl Iterator<Item = Result<Vec<u8>, std::io::Error>> + 'a {
        std::iter::from_fn(move || {
            let mut chunk = vec![0; chunk_size];
            match self.read(&mut chunk) {
                Ok(0) => None,
                Ok(nread) => {
                    chunk.truncate(nread);
                    Some(Ok(chunk))
                }
                Err(e) => Some(Err(e)),
            }
        })
    }

    /// Get a prefix of the body containing up to the given number of bytes.
    ///
    /// This is particularly useful when you only need to inspect the first few bytes of a body, or
    /// want to read an entire body up to a certain size limit to control memory consumption.
    ///
    /// Note that the length of the returned prefix may be shorter than the requested length if the
    /// length of the entire body is shorter.
    ///
    /// The returned [`Prefix`] value is a smart pointer wrapping a `&mut Vec<u8>`. You can use it
    /// as you would a [`&mut Vec<u8>`][`Vec`] or a [`&mut [u8]`][`std::slice`] to view or modify
    /// the contents of the prefix.
    ///
    /// When the [`Prefix`] is dropped, the prefix bytes are returned to the body, including any
    /// modifications that have been made. Because the prefix holds a mutable reference to the body,
    /// you may need to explicitly [`drop()`] the prefix to perform other operations on the
    /// body.
    ///
    /// If you do not need to return the prefix bytes to the body, use [`Prefix::take()`] to consume
    /// the prefix as an owned byte vector without writing it back.
    ///
    /// # Examples
    ///
    /// Checking whether the body starts with the [WebAssembly magic
    /// number](https://webassembly.github.io/spec/core/binary/modules.html#binary-module):
    ///
    /// ```no_run
    /// const MAGIC: &[u8] = b"\0asm";
    /// # let mut body = fastly::Body::from(MAGIC);
    /// let prefix = body.get_prefix_mut(MAGIC.len());
    /// if prefix.as_slice() == MAGIC {
    ///     println!("might be Wasm!");
    /// }
    /// ```
    ///
    /// Zero out the timestamp bytes in a [gzip header](https://en.wikipedia.org/wiki/Gzip#File_format):
    ///
    /// ```no_run
    /// # let mut body = fastly::Body::from(&[0x1f, 0x8b, 0x01, 0x00, 0xba, 0xc8, 0x4d, 0x20][..]);
    /// let mut prefix = body.get_prefix_mut(8);
    /// for i in 4..8 {
    ///     prefix[i] = 0;
    /// }
    /// ```
    ///
    /// Try to consume the body as a [JSON value][`serde_json::Value`], but only up to the first
    /// 4KiB. Note the use of `take()` to avoid writing the bytes back to the body unnecessarily:
    ///
    /// ```no_run
    /// # use serde_json::{json, to_writer};
    /// # let mut body = fastly::Body::new();
    /// # to_writer(&mut body, &json!({"hello": "world!" })).unwrap();
    /// let prefix = body.get_prefix_mut(4096).take();
    /// let json: serde_json::Value = serde_json::from_slice(&prefix).unwrap();
    /// ```
    pub fn get_prefix_mut(&mut self, length: usize) -> Prefix {
        self.try_get_prefix_mut(length).expect("body read failed")
    }

    /// Try to get a prefix of the body up to the given number of bytes.
    ///
    /// Unlike [`get_prefix_mut()`][`Self::get_prefix_mut()`], this method does not panic if an I/O
    /// error occurs.
    pub fn try_get_prefix_mut(&mut self, length: usize) -> std::io::Result<Prefix> {
        let mut buf = vec![];
        let nread = self
            .take(length.try_into().unwrap())
            .read_to_end(&mut buf)?;
        buf.truncate(nread);
        Ok(Prefix::new(buf, self))
    }

    /// Get a prefix of the body as a string containing up to the given number of bytes.
    ///
    /// This is particularly useful when you only need to inspect the first few characters of a body or
    /// want to read an entire body up to a certain size limit to control memory consumption.
    ///
    /// Note that the length of the returned prefix may be shorter than the requested length if the
    /// length of the entire body is shorter or if the requested length fell in the middle of a
    /// multi-byte UTF-8 codepoint.
    ///
    /// The returned [`PrefixString`] value is a smart pointer wrapping a `&mut String`. You can use
    /// it as you would a [`&mut String`][`String`] or a [`&mut str`][`std::str`] to view or modify
    /// the contents of the prefix.
    ///
    /// When the [`PrefixString`] is dropped, the prefix characters are returned to the body,
    /// including any modifications that have been made. Because the prefix holds a mutable
    /// reference to the body, you may need to explicitly [`drop()`] the prefix before performing
    /// other operations on the body.
    ///
    /// If you do not need to return the prefix characters to the body, use [`PrefixString::take()`] to
    /// consume the prefix as an owned string without writing it back.
    ///
    /// # Panics
    ///
    /// If the prefix contains invalid UTF-8 bytes, this function will panic. The exception to this
    /// is if the bytes are invalid because a multi-byte codepoint is cut off by the requested
    /// prefix length. In this case, the invalid bytes are left off the end of the prefix.
    ///
    /// To explicitly handle the possibility of invalid UTF-8 bytes, use
    /// [`try_get_prefix_str_mut()`][`Self::try_get_prefix_str_mut()`], which returns an error on
    /// failure rather than panicking.
    ///
    /// # Examples
    ///
    /// Check whether the body starts with the [M3U8 file header][m3u8]:
    ///
    /// ```no_run
    /// const HEADER: &str = "#EXTM3U";
    /// # let mut body = fastly::Body::from(HEADER);
    /// let prefix = body.get_prefix_str_mut(7);
    /// if prefix.as_str() == HEADER {
    ///     println!("might be an M3U8 file!");
    /// }
    /// ```
    ///
    /// Insert a new playlist entry before the first occurrence of `#EXTINF` in an [M3U8
    /// file][m3u8]:
    ///
    /// ```no_run
    /// # let mut body = fastly::Body::from("#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-VERSION:4\n#EXT-X-MEDIA-SEQUENCE:1\n#EXTINF:10.0,\nfileSequence1.ts\n");
    /// let mut prefix = body.get_prefix_str_mut(1024);
    /// let first_entry = prefix.find("#EXTINF").unwrap();
    /// prefix.insert_str(first_entry, "#EXTINF:10.0,\nnew_first_file.ts\n");
    /// ```
    ///
    /// Try to consume the body as a [JSON value][`serde_json::Value`], but only up to the first
    /// 4KiB. Note the use of `take()` to avoid writing the characters back to the body unnecessarily:
    ///
    /// ```no_run
    /// # use serde_json::{json, to_writer};
    /// # let mut body = fastly::Body::new();
    /// # to_writer(&mut body, &json!({"hello": "world!" })).unwrap();
    /// let prefix = body.get_prefix_str_mut(4096).take();
    /// let json: serde_json::Value = serde_json::from_str(&prefix).unwrap();
    /// ```
    ///
    /// [m3u8]: https://en.wikipedia.org/wiki/M3U#Extended_M3U
    pub fn get_prefix_str_mut(&mut self, length: usize) -> PrefixString {
        self.try_get_prefix_str_mut(length)
            .expect("UTF-8 error in body prefix")
    }

    /// Try to get a prefix of the body as a string containing up to the given number of bytes.
    ///
    /// Unlike [`get_prefix_str_mut()`][`Self::get_prefix_str_mut()`], this function does not panic
    /// when the prefix contains invalid UTF-8 bytes.
    pub fn try_get_prefix_str_mut(
        &mut self,
        length: usize,
    ) -> Result<PrefixString, std::str::Utf8Error> {
        let mut buf = vec![];
        let nread = self
            .take(length.try_into().unwrap())
            .read_to_end(&mut buf)
            .expect("body read failed");
        buf.truncate(nread);
        match String::from_utf8(buf) {
            Ok(string) => Ok(PrefixString::new(string, self)),
            Err(e) => {
                // Determine whether the error is due to the cutoff at the end or due to bad UTF-8
                // bytes. In either case, there may be bytes we want to put back onto the body.
                let err = e.utf8_error();
                let mut bytes = e.into_bytes();
                let (excess_bytes, result) = match err.error_len() {
                    None => {
                        // The error was due to a codepoint cut off at the end, so convert the valid
                        // part of the prefix and put the partial codepoint bytes back.
                        let end_bytes = bytes.split_off(err.valid_up_to());
                        let string = String::from_utf8(bytes)
                            .expect("expected only valid UTF-8 after splitting off bad codepoint");
                        (end_bytes, Ok(string))
                    }
                    Some(_) => {
                        // There were bad UTF-8 bytes within the prefix, so return a UTF-8 error and
                        // put all the bytes back.
                        (bytes, Err(err))
                    }
                };
                // Put invalid bytes back onto the body, if there are any.
                if !excess_bytes.is_empty() {
                    self.put_back_read_buf();
                    self.writer.get_mut().write_front(&excess_bytes);
                }
                result.map(move |string| PrefixString::new(string, self))
            }
        }
    }
}

// For these trait implementations we only implement the methods that the underlying buffered
// adaptors implement; the default implementations for the others will behave the same.
//
// The main bit of caution we must use here is that any read should be preceded by flushing the
// write buffer. `BufWriter` doesn't make any calls if its buffer is empty, so this isn't very
// expensive and could prevent unexpected results if a program is trying to read and write from the
// same body.
impl Read for Body {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.writer.flush()?;
        self.reader.read(buf)
    }

    fn read_vectored(&mut self, bufs: &mut [std::io::IoSliceMut]) -> std::io::Result<usize> {
        self.writer.flush()?;
        self.reader.read_vectored(bufs)
    }
}

impl BufRead for Body {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.writer.flush()?;
        self.reader.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.reader.consume(amt)
    }
}

impl Write for Body {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer.write(buf)
    }

    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        self.writer.write_vectored(bufs)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl From<BodyHandle> for Body {
    fn from(handle: BodyHandle) -> Self {
        // we clone the handle here in order to have an owned type for the reader and writer, but
        // this means we have to be careful that we don't make the aliasing observable from the
        // public interface
        let handle2 = unsafe { BodyHandle::from_u32(handle.as_u32()) };
        Self {
            reader: BufReader::new(BodyHandleWrapper::new(handle)),
            writer: BufWriter::new(handle2),
        }
    }
}

impl From<&str> for Body {
    fn from(s: &str) -> Self {
        BodyHandle::from(s).into()
    }
}

impl From<String> for Body {
    fn from(s: String) -> Self {
        BodyHandle::from(s).into()
    }
}

impl From<&[u8]> for Body {
    fn from(s: &[u8]) -> Self {
        BodyHandle::from(s).into()
    }
}

impl From<Vec<u8>> for Body {
    fn from(s: Vec<u8>) -> Self {
        BodyHandle::from(s).into()
    }
}

/// Smart pointer returned by [`Body::get_prefix_mut()`].
pub struct Prefix<'a> {
    /// The mutable prefix buffer, if it hasn't yet been taken.
    ///
    /// `Prefix` is always created with `Some(buf)`, and [`Prefix::take()`] is the only method that
    /// changes `self.buf` to `None`. That means the `unwrap`s in the other methods are safe.
    buf: Option<Vec<u8>>,
    body: &'a mut Body,
}

impl<'a> std::fmt::Debug for Prefix<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prefix")
            .field("buf", self.buf.as_ref().unwrap())
            .finish()
    }
}

impl<'a> Prefix<'a> {
    fn new(buf: Vec<u8>, body: &'a mut Body) -> Self {
        Self {
            buf: Some(buf),
            body,
        }
    }

    /// Return the prefix as a byte vector without writing it back to the `Body` from which it came.
    pub fn take(mut self) -> Vec<u8> {
        self.buf.take().unwrap()
    }
}

impl<'a> std::ops::Deref for Prefix<'a> {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        self.buf.as_ref().unwrap()
    }
}

impl<'a> std::ops::DerefMut for Prefix<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.buf.as_mut().unwrap()
    }
}

impl<'a> Drop for Prefix<'a> {
    fn drop(&mut self) {
        // Only put bytes back if `Prefix::take()` has not been called.
        if let Some(buf) = &self.buf {
            self.body.put_back_read_buf();
            self.body.writer.get_mut().write_front(&buf);
        }
    }
}

/// Smart pointer returned by [`Body::get_prefix_str_mut()`].
pub struct PrefixString<'a> {
    /// The mutable prefix buffer, if it hasn't yet been taken.
    ///
    /// `PrefixString` is always created with `Some(buf)`, and [`PrefixString::take()`] is the only
    /// method that changes `self.buf` to `None`. That means the `unwrap`s in the other methods are
    /// safe.
    buf: Option<String>,
    body: &'a mut Body,
}

impl<'a> std::fmt::Debug for PrefixString<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrefixString")
            .field("buf", self.buf.as_ref().unwrap())
            .finish()
    }
}

impl<'a> PrefixString<'a> {
    fn new(buf: String, body: &'a mut Body) -> Self {
        Self {
            buf: Some(buf),
            body,
        }
    }

    /// Return the prefix as a string without writing it back to the `Body` from which it came.
    pub fn take(mut self) -> String {
        self.buf.take().unwrap()
    }
}

impl<'a> std::ops::Deref for PrefixString<'a> {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        self.buf.as_ref().unwrap()
    }
}

impl<'a> std::ops::DerefMut for PrefixString<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.buf.as_mut().unwrap()
    }
}

impl<'a> Drop for PrefixString<'a> {
    fn drop(&mut self) {
        // Only put bytes back if `PrefixString::take()` has not been called.
        if let Some(buf) = &self.buf {
            self.body.put_back_read_buf();
            self.body.writer.get_mut().write_front(buf.as_bytes());
        }
    }
}

/// An internal wrapper used in `Body` to prevent closing the handle twice by
/// wrapping a `BodyHandle` in this type with a special Drop impl to prevent the
/// `BodyHandle` having it's destructor called. This type should not be used outside
/// of this module. All the function calls used by `Body` on the writer handle
/// are reimplemented for the wrapper and it just passes the function call to
/// the inner handle
#[repr(transparent)]
struct BodyHandleWrapper {
    handle: BodyHandle,
}

impl Drop for BodyHandleWrapper {
    fn drop(&mut self) {
        // In order to avoid doing a double free we replace the handle with an
        // invalid handle that doesn't point to anything. We then `mem::forget`
        // the original handle which is just a wrapper around `u32` and is left
        // on the stack that will get cleared out so we don't leak any memory.
        let handle = mem::replace(&mut self.handle, unsafe {
            BodyHandle::from_u32(fastly_shared::INVALID_BODY_HANDLE)
        });
        mem::forget(handle);
    }
}

impl BodyHandleWrapper {
    fn new(handle: BodyHandle) -> Self {
        Self { handle }
    }
}

impl Write for BodyHandleWrapper {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.handle.write(buf)
    }

    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        self.handle.write_vectored(bufs)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.handle.flush()
    }
}

impl Read for BodyHandleWrapper {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.handle.read(buf)
    }
}
