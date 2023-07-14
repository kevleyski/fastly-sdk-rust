//! HTTP bodies.

use crate::{
    abi::{self, FastlyStatus},
    error::{HandleError, HandleKind},
};
use fastly_shared::BodyWriteEnd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    io::{BufReader, Read, Write},
    mem::ManuallyDrop,
};

/// A low-level interface to HTTP bodies.
///
/// For most applications, you should use [`Body`][`crate::Body`] instead of this
/// interface. See the top-level [`handle`][`crate::handle`] documentation for more details.
///
/// This type implements [`Read`] to read bytes from the beginning of a body, and [`Write`] to write
/// to the end of a body. Note that these operations are unbuffered, unlike the same operations on
/// the higher-level [`Body`][`crate::Body`] type.
#[derive(Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct BodyHandle {
    pub(super) handle: u32,
}

/// A flag representing whether or not we have taken the client body.
pub(crate) static GOT_CLIENT_BODY: AtomicBool = AtomicBool::new(false);

impl BodyHandle {
    /// An invalid body handle.
    ///
    /// This is primarily useful to represent uninitialized values when using the interfaces in
    /// [`fastly_sys`].
    pub const INVALID: Self = Self {
        handle: fastly_shared::INVALID_BODY_HANDLE,
    };

    /// Returns `true` if the body handle is valid.
    pub const fn is_valid(&self) -> bool {
        !self.is_invalid()
    }

    /// Returns `true` if the body handle is invalid.
    pub const fn is_invalid(&self) -> bool {
        self.handle == fastly_shared::INVALID_BODY_HANDLE
    }

    /// Make a handle from its underlying representation.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub unsafe fn from_u32(handle: u32) -> Self {
        Self { handle }
    }

    /// Get the underlying representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub unsafe fn as_u32(&self) -> u32 {
        self.handle
    }

    /// Get a mutable reference to the underlying `u32` representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub unsafe fn as_u32_mut(&mut self) -> &mut u32 {
        &mut self.handle
    }

    /// Turn a handle into its representation without closing the underlying resource.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub fn into_u32(self) -> u32 {
        unsafe { ManuallyDrop::new(self).as_u32() }
    }

    /// Set `GOT_CLIENT_BODY` flag to show we've taken the client body.
    ///
    /// This will panic if the flag has already been set by someone else.
    pub(crate) fn set_got_client() {
        if GOT_CLIENT_BODY.swap(true, Ordering::SeqCst) {
            panic!("cannot get more than one handle to the client body per execution");
        }
    }

    /// Get a handle to the client request body.
    ///
    /// This handle may only be retrieved once per execution, either through this function or
    /// through [`client_request_and_body()`][`crate::handle::client_request_and_body()`].
    pub fn from_client() -> Self {
        Self::set_got_client();
        let mut handle = BodyHandle::INVALID;
        let status = unsafe {
            abi::fastly_http_req::body_downstream_get(std::ptr::null_mut(), handle.as_u32_mut())
        };
        match status.result().map(|_| handle) {
            Ok(h) if h.is_valid() => h,
            _ => panic!("fastly_http_req::body_downstream_get failed"),
        }
    }

    /// Acquire a new, empty body handle.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut handle = BodyHandle::INVALID;
        let status = unsafe { abi::fastly_http_body::new(handle.as_u32_mut()) };
        match status.result().map(|_| handle) {
            Ok(h) if h.is_valid() => h,
            _ => panic!("fastly_http_body::new failed"),
        }
    }

    /// Append another body onto the end of this body.
    ///
    #[doc = include_str!("../../../docs/snippets/body-append-constant-time.md")]
    ///
    /// The other body will no longer be valid after this call.
    pub fn append(&mut self, other: BodyHandle) {
        unsafe { abi::fastly_http_body::append(self.as_u32(), other.into_u32()) }
            .result()
            .expect("fastly_http_body::append failed")
    }

    /// Read the entirety of the body into a byte vector.
    ///
    #[doc = include_str!("../../../docs/snippets/buffers-body-handle.md")]
    pub fn into_bytes(self) -> Vec<u8> {
        let mut body = vec![];
        let mut bufread = BufReader::new(self);
        bufread
            .read_to_end(&mut body)
            .expect("fastly_http_body::read failed");
        body
    }

    /// Read the entirety of the body into a `String`, interpreting the bytes as UTF-8.
    ///
    #[doc = include_str!("../../../docs/snippets/buffers-body-handle.md")]
    ///
    /// # Panics
    ///
    #[doc = include_str!("../../../docs/snippets/panics-body-utf8.md")]
    pub fn into_string(self) -> String {
        let mut body = String::new();
        let mut bufread = BufReader::new(self);
        bufread
            .read_to_string(&mut body)
            .expect("fastly_http_body::read failed");
        body
    }

    /// Write a slice of bytes to the end of this body, and return the number of bytes written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::handle::BodyHandle;
    /// # let mut body = BodyHandle::new();
    /// # #[allow(deprecated)]
    /// body.write_bytes(&[0, 1, 2, 3]);
    /// ```
    #[deprecated(since = "0.9.3", note = "use std::io::Write::write() instead")]
    pub fn write_bytes(&mut self, bytes: &[u8]) -> usize {
        let mut nwritten = 0;
        let status = unsafe {
            abi::fastly_http_body::write(
                self.as_u32(),
                bytes.as_ptr(),
                bytes.len(),
                BodyWriteEnd::Back,
                &mut nwritten,
            )
        };
        status
            .result()
            .map(|_| nwritten)
            .expect("fastly_http_body::write failed")
    }

    /// Write a string slice to the end of this body, and return the number of bytes written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::handle::BodyHandle;
    /// # let mut body = BodyHandle::new();
    /// # #[allow(deprecated)]
    /// body.write_str("woof woof");
    /// ```
    #[deprecated(since = "0.9.3", note = "use std::io::Write::write() instead")]
    pub fn write_str(&mut self, string: &str) -> usize {
        #[allow(deprecated)]
        self.write_bytes(string.as_bytes())
    }

    pub(crate) fn write_front(&mut self, bytes: &[u8]) -> usize {
        let mut nwritten = 0;
        let status = unsafe {
            abi::fastly_http_body::write(
                self.as_u32(),
                bytes.as_ptr(),
                bytes.len(),
                BodyWriteEnd::Front,
                &mut nwritten,
            )
        };
        assert!(status.is_ok(), "fastly_http_body::write_front failed");
        assert!(
            nwritten == bytes.len(),
            "fastly_http_body::write_front didn't fully write"
        );
        nwritten
    }

    /// Close the BodyHandle by removing it from the host Session. This will close
    /// out streaming and non streaming bodies. Care must be taken to only
    /// close out the handle if the body is not being written to or read from
    /// anymore. If the handle has already been closed an error will be
    /// returned.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::io::Write;
    /// # use fastly::handle::BodyHandle;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut body = BodyHandle::new();
    /// body.write_all(b"You're already closed.")?;
    /// // The handle is not being used in a request and doesn't refer to any
    /// // response body so we can close this out
    /// body.close()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn close(self) -> Result<(), HandleError> {
        match unsafe { abi::fastly_http_body::close(self.into_u32()) } {
            FastlyStatus::OK => Ok(()),
            _ => Err(HandleError::ClosedHandle(HandleKind::Body)),
        }
    }
}

impl Read for BodyHandle {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::io::{Error, ErrorKind};

        let mut nread = 0;
        let status = unsafe {
            abi::fastly_http_body::read(self.as_u32(), buf.as_mut_ptr(), buf.len(), &mut nread)
        };
        match status {
            FastlyStatus::OK => Ok(nread),
            FastlyStatus::HTTPINCOMPLETE => {
                Err(Error::new(ErrorKind::UnexpectedEof, "incomplete HTTP body"))
            }
            _ => Err(Error::new(
                ErrorKind::Other,
                "fastly_http_body::read failed",
            )),
        }
    }
}

impl Write for BodyHandle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        use std::io::{Error, ErrorKind};

        let mut nwritten = 0;
        let status = unsafe {
            abi::fastly_http_body::write(
                self.as_u32(),
                buf.as_ptr(),
                buf.len(),
                BodyWriteEnd::Back,
                &mut nwritten,
            )
        };
        // similar to `BodyHandle::write_bytes()`, but doesn't panic on error
        match status {
            FastlyStatus::OK => Ok(nwritten),
            FastlyStatus::BADF => Err(Error::new(ErrorKind::InvalidInput, format!("{status:?}"))),
            _ => Err(Error::new(ErrorKind::Other, format!("{status:?}"))),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for BodyHandle {
    fn drop(&mut self) {
        if self.is_valid() {
            unsafe { abi::fastly_http_body::close(self.as_u32()) }
                .result()
                .expect("fastly_http_body::close failed");
        }
    }
}

impl From<&str> for BodyHandle {
    fn from(s: &str) -> Self {
        let mut handle = Self::new();
        handle
            .write_all(s.as_bytes())
            .expect("BodyHandle::from() write failed");
        handle
    }
}

impl From<String> for BodyHandle {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

impl From<&[u8]> for BodyHandle {
    fn from(s: &[u8]) -> Self {
        let mut handle = Self::new();
        handle
            .write_all(s)
            .expect("BodyHandle::from() write failed");
        handle
    }
}

impl From<Vec<u8>> for BodyHandle {
    fn from(s: Vec<u8>) -> Self {
        Self::from(s.as_slice())
    }
}
