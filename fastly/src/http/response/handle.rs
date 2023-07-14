use super::{FastlyResponseMetadata, Response};
use crate::abi::{self, FastlyStatus, MultiValueHostcallError};
use crate::error::{BufferSizeError, HandleError, HandleKind};
use crate::handle::{BodyHandle, StreamingBodyHandle};
use crate::http::request::SendError;
use crate::http::request::SendErrorCause;
use bytes::{BufMut, BytesMut};
use fastly_shared::{FramingHeadersMode, HttpKeepaliveMode};
use http::header::{HeaderName, HeaderValue};
use http::{StatusCode, Version};
use std::mem::ManuallyDrop;

// This import is just to get `RequestHandle` into scope for intradoc linking.
#[allow(unused)]
use crate::handle::RequestHandle;

/// A low-level interface to HTTP responses.
///
/// For most applications, you should use [`Response`] instead of this interface. See the top-level
/// [`handle`][`crate::handle`] documentation for more details.
///
/// # Sending to the client
///
/// Each execution of a Compute@Edge program may send a single response back to the client:
///
/// - [`ResponseHandle::send_to_client()`]
/// - [`ResponseHandle::stream_to_client`]
///
/// If no response is explicitly sent by the program, a default `200 OK` response is sent.
///
/// # Creation and conversion
///
/// Response handles can be created programmatically using [`ResponseHandle::new()`]
///
/// - [`Response::new()`]
/// - [`Response::from_body()`]
///
/// Response handles are also returned from backend requests:
///
/// - [`RequestHandle::send()`]
/// - [`RequestHandle::send_async()`]
/// - [`RequestHandle::send_async_streaming()`]
#[derive(Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct ResponseHandle {
    pub(crate) handle: u32,
}

impl ResponseHandle {
    /// An invalid handle.
    ///
    /// This is primarily useful to represent uninitialized values when using the interfaces in
    /// [`fastly_sys`].
    pub const INVALID: Self = ResponseHandle {
        handle: fastly_shared::INVALID_RESPONSE_HANDLE,
    };

    /// Returns `true` if the response handle is valid.
    pub const fn is_valid(&self) -> bool {
        !self.is_invalid()
    }

    /// Returns `true` if the response handle is invalid.
    pub const fn is_invalid(&self) -> bool {
        self.handle == fastly_shared::INVALID_RESPONSE_HANDLE
    }

    /// Get the underlying representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub(crate) fn as_u32(&self) -> u32 {
        self.handle
    }

    /// Get a mutable reference to the underlying `u32` representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub fn as_u32_mut(&mut self) -> &mut u32 {
        &mut self.handle
    }

    /// Turn a handle into its representation without closing the underlying resource.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub fn into_u32(self) -> u32 {
        ManuallyDrop::new(self).as_u32()
    }

    /// Acquire a new response handle.
    ///
    /// By default, the response will have a status code of `200 OK` and empty headers.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut handle = ResponseHandle::INVALID;
        let status = unsafe { abi::fastly_http_resp::new(handle.as_u32_mut()) };
        match status.result().map(|_| handle) {
            Ok(h) if h.is_valid() => h,
            _ => panic!("fastly_http_resp::new failed"),
        }
    }

    /// Read the response's header names via a buffer of the provided size.
    ///
    /// If there is a header name that is longer than `buf_size`, this will return a
    /// [`BufferSizeError`]; you can retry with a larger buffer size if necessary.
    pub fn get_header_names<'a>(
        &'a self,
        buf_size: usize,
    ) -> impl Iterator<Item = Result<HeaderName, BufferSizeError>> + 'a {
        self.get_header_names_impl(buf_size, Some(buf_size))
    }

    pub(crate) fn get_header_names_impl<'a>(
        &'a self,
        mut initial_buf_size: usize,
        max_buf_size: Option<usize>,
    ) -> impl Iterator<Item = Result<HeaderName, BufferSizeError>> + 'a {
        if let Some(max) = max_buf_size {
            initial_buf_size = std::cmp::min(initial_buf_size, max);
        }
        abi::MultiValueHostcall::new(
            b'\0',
            initial_buf_size,
            max_buf_size,
            move |buf, buf_size, cursor, ending_cursor, nwritten| unsafe {
                abi::fastly_http_resp::header_names_get(
                    self.as_u32(),
                    buf,
                    buf_size,
                    cursor,
                    ending_cursor,
                    nwritten,
                )
            },
        )
        .map(move |res| {
            use MultiValueHostcallError::{BufferTooSmall, ClosureError};
            match res {
                // we trust that the hostcall is giving us valid header bytes
                Ok(name_bytes) => Ok(HeaderName::from_bytes(&name_bytes).unwrap()),
                // return an error if the buffer was not large enough; this means there must be a
                // max buffer size
                Err(BufferTooSmall { needed_buf_size }) => Err(BufferSizeError::header_name(
                    max_buf_size
                        .expect("maximum buffer size must exist if a buffer size error occurs"),
                    needed_buf_size,
                )),
                // panic if the hostcall failed for some other reason
                Err(ClosureError(e)) => {
                    panic!("fastly_http_resp::header_names_get returned error: {:?}", e)
                }
            }
        })
    }

    /// Get a response's header values given a header name, via a buffer of the provided size.
    ///
    /// If there is a header value that is longer than the buffer, this will return a
    /// [`BufferSizeError`]; you can retry with a larger buffer size if necessary.
    ///
    /// ### Examples
    ///
    /// Collect all the header values into a [`Vec`]:
    ///
    /// ```no_run
    /// # use fastly::error::Error;
    /// # use fastly::handle::ResponseHandle;
    /// # use http::header::{HeaderName, HeaderValue};
    /// #
    /// # fn main() -> Result<(), Error> {
    /// # let response = ResponseHandle::new();
    /// let name = HeaderName::from_static("My-App-Header");
    /// let buf_size = 128;
    /// let header_values: Vec<HeaderValue> = response
    ///     .get_header_values(&name, buf_size)
    ///     .collect::<Result<Vec<HeaderValue>, _>>()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// To try again with a larger buffer if the first call fails, you can use
    /// [`unwrap_or_else()`][`Result::unwrap_or_else()`]:
    ///
    /// ```no_run
    /// # use fastly::error::BufferSizeError;
    /// # use fastly::handle::ResponseHandle;
    /// # use http::header::{HeaderName, HeaderValue};
    /// # let response = ResponseHandle::new();
    /// let name = HeaderName::from_static("My-App-Header");
    /// let buf_size = 128;
    ///
    /// // Collect header values into a `Vec<HeaderValue>`, with a buffer size of `128`.
    /// // If the first call fails, print our error and then try to collect header values
    /// // again. The second call will use a larger buffer size of `1024`.
    /// let header_values: Vec<HeaderValue> = response
    ///     .get_header_values(&name, buf_size)
    ///     .collect::<Result<_, _>>()
    ///     .unwrap_or_else(|err: BufferSizeError| {
    ///         eprintln!("buffer size error: {}", err);
    ///         let larger_buf_size = 1024;
    ///         response
    ///             .get_header_values(&name, larger_buf_size)
    ///             .collect::<Result<_, _>>()
    ///             .unwrap()
    ///     });
    /// ```
    pub fn get_header_values<'a>(
        &'a self,
        name: &'a HeaderName,
        max_len: usize,
    ) -> impl Iterator<Item = Result<HeaderValue, BufferSizeError>> + 'a {
        self.get_header_values_impl(name, max_len, Some(max_len))
    }

    pub(crate) fn get_header_values_impl<'a>(
        &'a self,
        name: &'a HeaderName,
        mut initial_buf_size: usize,
        max_buf_size: Option<usize>,
    ) -> impl Iterator<Item = Result<HeaderValue, BufferSizeError>> + 'a {
        if let Some(max) = max_buf_size {
            initial_buf_size = std::cmp::min(initial_buf_size, max);
        }
        abi::MultiValueHostcall::new(
            b'\0',
            initial_buf_size,
            max_buf_size,
            move |buf, buf_size, cursor, ending_cursor, nwritten| unsafe {
                let name: &[u8] = name.as_ref();
                abi::fastly_http_resp::header_values_get(
                    self.as_u32(),
                    name.as_ptr(),
                    name.len(),
                    buf,
                    buf_size,
                    cursor,
                    ending_cursor,
                    nwritten,
                )
            },
        )
        .map(move |res| {
            use MultiValueHostcallError::{BufferTooSmall, ClosureError};
            match res {
                Ok(value_bytes) => {
                    let header_value =
                        unsafe { HeaderValue::from_maybe_shared_unchecked(value_bytes) };
                    Ok(header_value)
                }
                Err(BufferTooSmall { needed_buf_size }) => Err(BufferSizeError::header_value(
                    max_buf_size
                        .expect("maximum buffer size must exist if a buffer size error occurs"),
                    needed_buf_size,
                )),
                Err(ClosureError(e)) => panic!(
                    "fastly_http_resp::header_values_get returned error: {:?}",
                    e
                ),
            }
        })
    }

    /// Set the values for the given header name, replacing any headers that previously existed for
    /// that name.
    pub fn set_header_values<'a, I>(&mut self, name: &HeaderName, values: I)
    where
        I: IntoIterator<Item = &'a HeaderValue>,
    {
        // build a buffer of all the values, each terminated by a nul byte
        let mut buf = vec![];
        for value in values {
            buf.put(value.as_bytes());
            buf.put_u8(b'\0');
        }

        let name: &[u8] = name.as_ref();
        unsafe {
            abi::fastly_http_resp::header_values_set(
                self.as_u32(),
                name.as_ptr(),
                name.len(),
                buf.as_ptr(),
                buf.len(),
            )
        }
        .result()
        .expect("fastly_http_resp::header_values_set failed");
    }

    #[doc = include_str!("../../../docs/snippets/handle-get-header-value.md")]
    pub fn get_header_value(
        &self,
        name: &HeaderName,
        max_len: usize,
    ) -> Result<Option<HeaderValue>, BufferSizeError> {
        let name: &[u8] = name.as_ref();
        let mut buf = BytesMut::with_capacity(max_len);
        let mut nwritten = 0;
        let status = unsafe {
            abi::fastly_http_resp::header_value_get(
                self.as_u32(),
                name.as_ptr(),
                name.len(),
                buf.as_mut_ptr(),
                buf.capacity(),
                &mut nwritten,
            )
        };
        match status.result().map(|_| nwritten) {
            Ok(nwritten) => {
                assert!(nwritten <= buf.capacity(), "hostcall wrote too many bytes");
                unsafe {
                    buf.set_len(nwritten);
                }
                // we trust that the hostcall is giving us valid header bytes
                let value = HeaderValue::from_bytes(&buf).expect("bytes from host are valid");
                Ok(Some(value))
            }
            Err(FastlyStatus::INVAL) => Ok(None),
            Err(FastlyStatus::BUFLEN) => Err(BufferSizeError::header_value(max_len, nwritten)),
            _ => panic!("fastly_http_resp::header_value_get returned error"),
        }
    }

    /// Set a response header to the given value, discarding any previous values for the given
    /// header name.
    pub fn insert_header(&mut self, name: &HeaderName, value: &HeaderValue) {
        let name_bytes: &[u8] = name.as_ref();
        let value_bytes: &[u8] = value.as_ref();
        unsafe {
            abi::fastly_http_resp::header_insert(
                self.as_u32(),
                name_bytes.as_ptr(),
                name_bytes.len(),
                value_bytes.as_ptr(),
                value_bytes.len(),
            )
        }
        .result()
        .expect("fastly_http_resp::header_insert returned error");
    }

    /// Add a response header with given value.
    ///
    /// Unlike [`insert_header()`][`Self::insert_header()`], this does not discard existing values
    /// for the same header name.
    pub fn append_header(&mut self, name: &HeaderName, value: &HeaderValue) {
        let name_bytes: &[u8] = name.as_ref();
        let value_bytes: &[u8] = value.as_ref();
        unsafe {
            abi::fastly_http_resp::header_append(
                self.as_u32(),
                name_bytes.as_ptr(),
                name_bytes.len(),
                value_bytes.as_ptr(),
                value_bytes.len(),
            )
        }
        .result()
        .expect("fastly_http_resp::header_append returned error");
    }

    /// Remove all response headers of the given name, and return whether any headers were removed.
    pub fn remove_header(&mut self, name: &HeaderName) -> bool {
        let name_bytes: &[u8] = name.as_ref();
        let status = unsafe {
            abi::fastly_http_resp::header_remove(
                self.as_u32(),
                name_bytes.as_ptr(),
                name_bytes.len(),
            )
        };
        match status.result() {
            Ok(_) => true,
            Err(FastlyStatus::INVAL) => false,
            _ => panic!("fastly_http_resp::header_remove returned error"),
        }
    }

    /// Set the HTTP status code of this response.
    pub fn set_status(&mut self, status: StatusCode) {
        unsafe { abi::fastly_http_resp::status_set(self.as_u32(), status.as_u16()) }
            .result()
            .expect("fastly_http_resp::status_set returned error")
    }

    /// Get the HTTP status code of this response.
    pub fn get_status(&self) -> StatusCode {
        let mut status = 0;
        let fastly_status =
            unsafe { abi::fastly_http_resp::status_get(self.as_u32(), &mut status) };
        match fastly_status.result().map(|_| status) {
            Ok(status) => StatusCode::from_u16(status).expect("invalid http status"),
            _ => panic!("fastly_http_resp::status_get failed"),
        }
    }

    /// Get the HTTP version of this response.
    pub fn get_version(&self) -> Version {
        let mut version = 0;
        let status = unsafe { abi::fastly_http_resp::version_get(self.as_u32(), &mut version) };
        if status.is_err() {
            panic!("fastly_http_resp::version_get failed");
        } else {
            abi::HttpVersion::try_from(version)
                .map(Into::into)
                .expect("invalid http version")
        }
    }

    /// Set the HTTP version of this response.
    pub fn set_version(&mut self, v: Version) {
        unsafe {
            abi::fastly_http_resp::version_set(self.as_u32(), abi::HttpVersion::from(v) as u32)
        }
        .result()
        .expect("fastly_http_resp::version_get failed");
    }

    /// Immediately begin sending this response downstream to the client with the given body.
    pub fn send_to_client(self, body: BodyHandle) {
        unsafe {
            abi::fastly_http_resp::send_downstream(self.into_u32(), body.into_u32(), false as u32)
        }
        .result()
        .expect("fastly_http_resp::send_downstream failed");
    }

    /// Immediately begin sending this response downstream to the client, and return a
    /// [`StreamingBodyHandle`] that can accept further data to send.
    pub fn stream_to_client(self, body: BodyHandle) -> StreamingBodyHandle {
        let status = unsafe {
            abi::fastly_http_resp::send_downstream(self.into_u32(), body.as_u32(), true as u32)
        };
        let streaming_body_handle = StreamingBodyHandle::from_body_handle(body);
        status
            .result()
            .map(|_| streaming_body_handle)
            .expect("fastly_http_resp::send_downstream failed")
    }

    /// Sets the way that framing headers are determined for this response.
    pub fn set_framing_headers_mode(&mut self, mode: FramingHeadersMode) {
        unsafe { abi::fastly_http_resp::framing_headers_mode_set(self.as_u32(), mode) }
            .result()
            .expect("fastly_http_resp::framing_headers_mode_set failed")
    }

    #[doc(hidden)]
    /// Sets whether the client is encouraged to stop using the current connection and to open
    /// a new one for the next request. If this method returns an error, the requested keepalive
    /// mode may not be respected, but the response will otherwise be intact.
    pub fn set_http_keepalive_mode(&mut self, mode: HttpKeepaliveMode) -> Result<(), FastlyStatus> {
        unsafe { abi::fastly_http_resp::http_keepalive_mode_set(self.as_u32(), mode) }.result()
    }

    /// Close the ResponseHandle by removing it from the host Session. If the handle has already
    /// been closed an error will be returned. A ResponseHandle is only consumed
    /// when you send a response to a client or stream one to a client. You
    /// should call close only if you don't intend to use that Response anymore.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::handle::ResponseHandle;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let response = ResponseHandle::new();
    /// // The handle is not being used so we can close it out without any
    /// // trouble
    /// response.close()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn close(self) -> Result<(), HandleError> {
        match unsafe { abi::fastly_http_resp::close(self.as_u32()) } {
            FastlyStatus::OK => Ok(()),
            _ => Err(HandleError::ClosedHandle(HandleKind::Response)),
        }
    }
}

pub(crate) fn handles_to_response(
    resp_handle: ResponseHandle,
    resp_body_handle: BodyHandle,
    metadata: FastlyResponseMetadata,
) -> Result<Response, SendError> {
    match Response::from_handles(resp_handle, resp_body_handle) {
        Ok(mut resp) => {
            resp.set_fastly_metadata(metadata);
            Ok(resp)
        }
        Err(bse) => Err(SendError::from_resp_metadata(
            metadata,
            SendErrorCause::BufferSize(bse),
        )),
    }
}

impl Drop for ResponseHandle {
    fn drop(&mut self) {
        if self.is_valid() {
            unsafe { abi::fastly_http_resp::close(self.as_u32()) }
                .result()
                .expect("fastly_http_resp::close failed");
        }
    }
}
