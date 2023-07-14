use fastly_shared::FastlyStatus;

use crate::error::HandleError;

use super::super::handle::BodyHandle;
use std::io::Write;
use std::mem::ManuallyDrop;

/// A low-level interface to a streaming HTTP body.
///
/// The interface to this type is very similar to [`BodyHandle`], however it is write-only, and can
/// only be created as a result of calling
/// [`ResponseHandle::send_to_client()`][`crate::handle::ResponseHandle::send_to_client()`] or
/// [`RequestHandle::send_async_streaming()`][`crate::handle::RequestHandle::send_async_streaming()`].
///
/// This type implements [`Write`] to write to the end of a body. Note that these operations are
/// unbuffered, unlike the same operations on the higher-level [`Body`][`crate::Body`] type.
///
/// A streaming body handle will be automatically aborted if it goes out of scope without calling
/// [`finish()`][`Self::finish()`].
#[derive(Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
#[must_use = "streaming body handles must be `.finish()`ed"]
pub struct StreamingBodyHandle {
    // The `BodyHandle` is wrapped in `ManuallyDrop` so that we do not automatically call `close()`
    // when dropping a streaming body. `close()` must only be called when the user affirmatively
    // `finish()`es the streaming body.
    handle: ManuallyDrop<BodyHandle>,
}

impl StreamingBodyHandle {
    /// Finish writing to a streaming body handle.
    pub fn finish(self) -> Result<(), HandleError> {
        match unsafe { fastly_sys::fastly_http_body::close(self.into_u32()) } {
            FastlyStatus::OK => Ok(()),
            FastlyStatus::BADF => Err(HandleError::InvalidHandle),
            other => panic!(
                "unexpected error from `fastly_http_body::close`: {:?}; \
                             please report this as a bug",
                other
            ),
        }
    }

    /// Make a streaming body handle from a non-streaming handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub fn from_body_handle(body_handle: BodyHandle) -> Self {
        Self {
            handle: ManuallyDrop::new(body_handle),
        }
    }

    /// Get the underlying representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub unsafe fn as_u32(&self) -> u32 {
        self.handle.as_u32()
    }

    /// Turn a handle into its representation without closing the underlying resource.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub fn into_u32(self) -> u32 {
        unsafe { ManuallyDrop::new(self).as_u32() }
    }

    /// Append another body onto the end of this body.
    ///
    #[doc = include_str!("../../../../docs/snippets/body-append-constant-time.md")]
    ///
    /// The other body will no longer be valid after this call.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::handle::{BodyHandle, ResponseHandle};
    /// # let response_handle = ResponseHandle::new();
    /// # let other_body = BodyHandle::new();
    /// let mut streaming_body = response_handle.stream_to_client(BodyHandle::new());
    /// streaming_body.append(other_body);
    /// ```
    pub fn append(&mut self, other: BodyHandle) {
        self.handle.append(other)
    }

    /// Write a slice of bytes to the end of this streaming body, and return the number of bytes written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::handle::{BodyHandle, ResponseHandle};
    /// # let response_handle = ResponseHandle::new();
    /// let mut streaming_body = response_handle.stream_to_client(BodyHandle::new());
    /// # #[allow(deprecated)]
    /// streaming_body.write_bytes(&[0, 1, 2, 3]);
    /// ```
    #[deprecated(since = "0.9.3", note = "use std::io::Write::write() instead")]
    pub fn write_bytes(&mut self, bytes: &[u8]) -> usize {
        #[allow(deprecated)]
        self.handle.write_bytes(bytes)
    }

    /// Write a string slice to the end of this streaming body, and return the number of bytes written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::handle::{BodyHandle, ResponseHandle};
    /// # let response_handle = ResponseHandle::new();
    /// let mut streaming_body = response_handle.stream_to_client(BodyHandle::new());
    /// # #[allow(deprecated)]
    /// streaming_body.write_str("woof woof");
    /// ```
    #[deprecated(since = "0.9.3", note = "use std::io::Write::write() instead")]
    pub fn write_str(&mut self, string: &str) -> usize {
        #[allow(deprecated)]
        self.write_bytes(string.as_bytes())
    }
}

impl Write for StreamingBodyHandle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.handle.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.handle.flush()
    }
}
