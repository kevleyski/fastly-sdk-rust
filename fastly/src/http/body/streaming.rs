pub(crate) mod handle;

use self::handle::StreamingBodyHandle;
use super::Body;
use std::io::{BufWriter, Write};

/// A streaming HTTP body that can be written to, or appended to from another body.
///
/// The interface to this type is very similar to `Body`, however it is write-only, and can only be
/// created as a result of calling
/// [`Response::stream_to_client()`][`crate::Response::stream_to_client()`] or
/// [`Request::send_async_streaming()`][`crate::Request::send_async_streaming()`].
///
/// The most efficient way to write the body is through the [`Write`] implementation. Writes are
/// buffered, and automatically flushed, but you can call [`Write::flush()`] to explicitly flush the
/// buffer and cause a new chunk to be written to the client.
///
/// A streaming body handle will be automatically aborted if it goes out of scope without calling
/// [`finish()`][`Self::finish()`].
#[must_use = "streaming bodies must be `.finish()`ed"]
pub struct StreamingBody {
    writer: BufWriter<StreamingBodyHandle>,
}

impl StreamingBody {
    /// Finish writing to a streaming body handle.
    pub fn finish(self) -> std::io::Result<()> {
        self.writer
            .into_inner()?
            .finish()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    // this is not exported, since misuse can lead to data getting dropped or appearing out of order
    fn handle(&mut self) -> &mut StreamingBodyHandle {
        self.writer.get_mut()
    }

    /// Append a body onto the end of this streaming body.
    ///
    #[doc = include_str!("../../../docs/snippets/body-append-constant-time.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::{Body, Response};
    /// # let beresp = Response::new();
    /// # let other_body = Body::new();
    /// let mut streaming_body = beresp.stream_to_client();
    /// streaming_body.append(other_body);
    /// ```
    pub fn append(&mut self, other: Body) {
        // flush the write buffer of the destination body, so that we can use the append method on
        // the underlying handles
        self.writer.flush().expect("fastly_http_body::write failed");
        self.handle().append(other.into_handle())
    }

    /// Write a slice of bytes to the end of this streaming body, and return the number of bytes written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # let resp = fastly::Response::new();
    /// let mut streaming_body = resp.stream_to_client();
    /// streaming_body.write_bytes(&[0, 1, 2, 3]);
    /// ```
    pub fn write_bytes(&mut self, bytes: &[u8]) -> usize {
        self.writer
            .write(bytes)
            .expect("fastly_http_body::write failed")
    }

    /// Write a string slice to the end of this streaming body, and return the number of bytes
    /// written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # let resp = fastly::Response::new();
    /// let mut streaming_body = resp.stream_to_client();
    /// streaming_body.write_str("woof woof");
    /// ```
    pub fn write_str(&mut self, string: &str) -> usize {
        self.write_bytes(string.as_ref())
    }
}

impl From<StreamingBodyHandle> for StreamingBody {
    fn from(handle: StreamingBodyHandle) -> Self {
        Self {
            writer: BufWriter::new(handle),
        }
    }
}

// This trait implementation is much simpler than those of `Body`, since we don't have to manage
// multiple buffers. It's just a passthrough to the methods defined on `BufWriter`.
impl Write for StreamingBody {
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
