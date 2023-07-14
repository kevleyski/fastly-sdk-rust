pub use fastly_shared::{CacheOverride, ClientCertVerifyResult, FramingHeadersMode};
pub use fastly_sys::ContentEncodings;

use super::pending::handle::PendingRequestHandle;
use crate::abi::{self, FastlyStatus, MultiValueHostcallError};
use crate::error::{BufferSizeError, HandleError, HandleKind};
use crate::handle::{BodyHandle, ResponseHandle, StreamingBodyHandle};
use crate::http::request::SendErrorCause;
use bytes::{BufMut, BytesMut};
use http::header::{HeaderName, HeaderValue};
use http::{Method, Version};
use lazy_static::lazy_static;
use std::mem::ManuallyDrop;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use url::Url;

// This import is just to get `Request` into scope for intradoc linking.
#[allow(unused)]
use super::Request;

/// The low-level interface to HTTP requests.
///
/// For most applications, you should use [`Request`] instead of this
/// interface. See the top-level [`handle`][`crate::handle`] documentation for more details.
///
/// # Getting the client request
///
/// Call [`RequestHandle::from_client()`] to get the client request being handled by this execution
/// of the Compute@Edge program.
///
/// # Creation and conversion
///
/// New requests can be created programmatically with [`RequestHandle::new()`]. In addition, you can
/// convert to and from [`Request`] using [`Request::from_handles()`] and
/// [`Request::into_handles()`].
///
/// # Sending backend requests
///
/// Requests can be sent to a backend in blocking or asynchronous fashion using
/// [`send()`][`Self::send()`], [`send_async()`][`Self::send_async()`], or
/// [`send_async_streaming()`][`Self::send_async_streaming()`].
#[derive(Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct RequestHandle {
    handle: u32,
}

/// A flag representing whether or not the request has been taken from the client.
pub(crate) static GOT_CLIENT_REQ: AtomicBool = AtomicBool::new(false);

impl RequestHandle {
    /// An invalid handle.
    ///
    /// This is primarily useful to represent uninitialized values when using the interfaces in
    /// [`fastly_sys`].
    pub const INVALID: Self = RequestHandle {
        handle: fastly_shared::INVALID_REQUEST_HANDLE,
    };

    /// Returns `true` if the request handle is valid.
    pub const fn is_valid(&self) -> bool {
        !self.is_invalid()
    }

    /// Returns `true` if the request handle is invalid.
    pub const fn is_invalid(&self) -> bool {
        self.handle == fastly_shared::INVALID_REQUEST_HANDLE
    }

    /// Get the underlying representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub fn as_u32(&self) -> u32 {
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
    pub(crate) fn into_u32(self) -> u32 {
        ManuallyDrop::new(self).as_u32()
    }

    /// Set `GOT_CLIENT_REQ` flag to show we've taken the client request.
    ///
    /// This will panic if the flag has already been set by someone else.
    pub(crate) fn set_got_client() {
        if GOT_CLIENT_REQ.swap(true, Ordering::SeqCst) {
            panic!("cannot get more than one handle to the client request per execution",);
        }
    }

    /// Get a handle to the client request being handled by this execution of the Compute@Edge program.
    ///
    /// # Panics
    ///
    /// This method panics if the client request has already been retrieved by this method,
    /// [`client_request_and_body()`], or [`Request::from_client()`].
    pub fn from_client() -> Self {
        Self::set_got_client();
        let mut handle = RequestHandle::INVALID;
        let status = unsafe {
            abi::fastly_http_req::body_downstream_get(handle.as_u32_mut(), std::ptr::null_mut())
        };
        match status.result().map(|_| handle) {
            Ok(h) if h.is_valid() => h,
            _ => panic!("fastly_http_req::body_downstream_get failed"),
        }
    }

    /// Acquire a new request handle.
    ///
    /// By default, the request will have a `GET` method, a URL of `/`, and empty headers.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut handle = RequestHandle::INVALID;
        let status = unsafe { abi::fastly_http_req::new(handle.as_u32_mut()) };
        match status.result().map(|_| handle) {
            Ok(h) if h.is_valid() => h,
            _ => panic!("fastly_http_req::new failed"),
        }
    }

    /// Read the request's header names via a buffer of the provided size.
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
                abi::fastly_http_req::header_names_get(
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
                // return an error if the buffer was not large enough
                Err(BufferTooSmall { needed_buf_size }) => Err(BufferSizeError::header_name(
                    max_buf_size
                        .expect("maximum buffer size must exist if a buffer size error occurs"),
                    needed_buf_size,
                )),
                // panic if the hostcall failed for some other reason
                Err(ClosureError(e)) => {
                    panic!("fastly_http_req::header_names_get returned error: {:?}", e)
                }
            }
        })
    }

    /// Get the header values for the given name via a buffer of the provided size.
    ///
    /// If there is a header value that is longer than the buffer, this will return a
    /// [`BufferSizeError`]; you can retry with a larger buffer size if necessary.
    ///
    /// # Examples
    ///
    /// Collect all the header values into a [`Vec`]:
    ///
    /// ```no_run
    /// # use fastly::error::Error;
    /// # use fastly::handle::RequestHandle;
    /// # use http::header::{HeaderName, HeaderValue};
    /// #
    /// # fn main() -> Result<(), Error> {
    /// # let request = RequestHandle::new();
    /// let name = HeaderName::from_static("My-App-Header");
    /// let buf_size = 128;
    /// let header_values: Vec<HeaderValue> = request
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
    /// # use fastly::handle::RequestHandle;
    /// # use http::header::{HeaderName, HeaderValue};
    /// # let request = RequestHandle::new();
    /// let name = HeaderName::from_static("My-App-Header");
    /// let buf_size = 128;
    ///
    /// // Collect header values into a `Vec<HeaderValue>`, with a buffer size of `128`.
    /// // If the first call fails, print our error and then try to collect header values
    /// // again. The second call will use a larger buffer size of `1024`.
    /// let header_values: Vec<HeaderValue> = request
    ///     .get_header_values(&name, buf_size)
    ///     .collect::<Result<_, _>>()
    ///     .unwrap_or_else(|err: BufferSizeError| {
    ///         let larger_buf_size = 1024;
    ///         request
    ///             .get_header_values(&name, larger_buf_size)
    ///             .collect::<Result<_, _>>()
    ///             .unwrap()
    ///     });
    /// ```
    pub fn get_header_values<'a>(
        &'a self,
        name: &'a HeaderName,
        buf_size: usize,
    ) -> impl Iterator<Item = Result<HeaderValue, BufferSizeError>> + 'a {
        self.get_header_values_impl(name, buf_size, Some(buf_size))
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
                abi::fastly_http_req::header_values_get(
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
                // we trust that the hostcall is giving us valid header bytes
                Ok(value_bytes) => {
                    let header_value =
                        unsafe { HeaderValue::from_maybe_shared_unchecked(value_bytes) };
                    Ok(header_value)
                }
                // return an error if the buffer was not large enough
                Err(BufferTooSmall { needed_buf_size }) => Err(BufferSizeError::header_value(
                    max_buf_size
                        .expect("maximum buffer size must exist if a buffer size error occurs"),
                    needed_buf_size,
                )),
                // panic if the hostcall failed for some other reason
                Err(ClosureError(e)) => {
                    panic!("fastly_http_req::header_values_get returned error: {:?}", e)
                }
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
            abi::fastly_http_req::header_values_set(
                self.as_u32(),
                name.as_ptr(),
                name.len(),
                buf.as_ptr(),
                buf.len(),
            )
        }
        .result()
        .expect("fastly_http_req::header_values_set failed");
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
            abi::fastly_http_req::header_value_get(
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
            _ => panic!("fastly_http_req::header_value_get returned error"),
        }
    }

    /// Set a request header to the given value, discarding any previous values for the given header
    /// name.
    pub fn insert_header(&mut self, name: &HeaderName, value: &HeaderValue) {
        let name_bytes: &[u8] = name.as_ref();
        let value_bytes: &[u8] = value.as_ref();
        let status = unsafe {
            abi::fastly_http_req::header_insert(
                self.as_u32(),
                name_bytes.as_ptr(),
                name_bytes.len(),
                value_bytes.as_ptr(),
                value_bytes.len(),
            )
        };
        if status.is_err() {
            panic!("fastly_http_req::header_insert returned error");
        }
    }

    /// Add a request header with given value.
    ///
    /// Unlike [`insert_header()`][`Self::insert_header()`], this does not discard existing values
    /// for the same header name.
    pub fn append_header(&mut self, name: &HeaderName, value: &HeaderValue) {
        let name_bytes: &[u8] = name.as_ref();
        let value_bytes: &[u8] = value.as_ref();
        unsafe {
            abi::fastly_http_req::header_append(
                self.as_u32(),
                name_bytes.as_ptr(),
                name_bytes.len(),
                value_bytes.as_ptr(),
                value_bytes.len(),
            )
        }
        .result()
        .expect("fastly_http_req::header_append returned error");
    }

    /// Remove all request headers of the given name, and return whether any headers were removed.
    pub fn remove_header(&mut self, name: &HeaderName) -> bool {
        let name_bytes: &[u8] = name.as_ref();
        let status = unsafe {
            abi::fastly_http_req::header_remove(
                self.as_u32(),
                name_bytes.as_ptr(),
                name_bytes.len(),
            )
        };
        match status.result() {
            Ok(_) => true,
            Err(FastlyStatus::INVAL) => false,
            _ => panic!("fastly_http_req::header_remove returned error"),
        }
    }

    /// Get the HTTP version of this request.
    pub fn get_version(&self) -> Version {
        let mut version = 0;
        let status = unsafe { abi::fastly_http_req::version_get(self.as_u32(), &mut version) };
        if status.is_err() {
            panic!("fastly_http_req::version_get failed");
        } else {
            abi::HttpVersion::try_from(version)
                .map(Into::into)
                .expect("HTTP version must be valid")
        }
    }

    /// Set the HTTP version of this request.
    pub fn set_version(&mut self, v: Version) {
        unsafe {
            abi::fastly_http_req::version_set(self.as_u32(), abi::HttpVersion::from(v) as u32)
        }
        .result()
        .expect("fastly_http_req::version_get failed");
    }

    /// Get the request method.
    ///
    /// If the method is longer than `max_length`, this will return a [`BufferSizeError`]; you can
    /// retry with a larger buffer size if necessary.
    pub fn get_method(&self, max_length: usize) -> Result<Method, BufferSizeError> {
        let mut method_bytes = Vec::with_capacity(max_length);
        let mut nwritten = 0;
        let status = unsafe {
            abi::fastly_http_req::method_get(
                self.as_u32(),
                method_bytes.as_mut_ptr(),
                method_bytes.capacity(),
                &mut nwritten,
            )
        };
        match status.result() {
            Ok(_) => {
                assert!(
                    nwritten <= method_bytes.capacity(),
                    "fastly_http_req::method_get wrote too many bytes"
                );
                unsafe {
                    method_bytes.set_len(nwritten);
                }
                Ok(Method::from_bytes(&method_bytes).expect("HTTP method must be valid"))
            }
            Err(FastlyStatus::BUFLEN) => Err(BufferSizeError::http_method(max_length, nwritten)),
            _ => panic!("fastly_http_req::method_get failed"),
        }
    }

    pub(crate) fn get_method_impl(
        &self,
        mut initial_buf_size: usize,
        max_buf_size: Option<usize>,
    ) -> Result<Method, BufferSizeError> {
        if let Some(max) = max_buf_size {
            initial_buf_size = std::cmp::min(initial_buf_size, max);
        }
        match self.get_method(initial_buf_size) {
            Ok(method) => Ok(method),
            Err(mut err) => {
                if let Some(max) = max_buf_size {
                    // if there's a max size, enforce it
                    if err.needed_buf_size <= max {
                        self.get_method(err.needed_buf_size)
                    } else {
                        // report the maximum that was exceeded, not what we tried
                        err.buf_size = max;
                        Err(err)
                    }
                } else {
                    // otherwise just get as much as is needed
                    self.get_method(err.needed_buf_size)
                }
            }
        }
    }

    /// Set the request method.
    pub fn set_method(&self, method: &Method) {
        let method_bytes = method.as_str().as_bytes();
        unsafe {
            abi::fastly_http_req::method_set(
                self.as_u32(),
                method_bytes.as_ptr(),
                method_bytes.len(),
            )
        }
        .result()
        .expect("fastly_http_req::method_set failed");
    }

    /// Get the request URL.
    ///
    /// If the URL is longer than `max_length`, this will return a [`BufferSizeError`]; you can
    /// retry with a larger buffer size if necessary.
    pub fn get_url(&self, max_length: usize) -> Result<Url, BufferSizeError> {
        let mut url_bytes = BytesMut::with_capacity(max_length);
        let mut nwritten = 0;
        let status = unsafe {
            abi::fastly_http_req::uri_get(
                self.as_u32(),
                url_bytes.as_mut_ptr(),
                url_bytes.capacity(),
                &mut nwritten,
            )
        };
        match status.result() {
            Ok(_) => {
                assert!(
                    nwritten <= url_bytes.capacity(),
                    "fastly_http_req::uri_get wrote too many bytes"
                );
                unsafe {
                    url_bytes.set_len(nwritten);
                }
                // TODO ACF 2020-08-28: use the `TryFrom<&[u8]>` impl once this change is merged and
                // released: https://github.com/servo/rust-url/pull/638
                let url_str =
                    std::str::from_utf8(&url_bytes).expect("host provided invalid request url");
                let url = Url::parse(url_str).expect("host provided invalid request url");
                Ok(url)
            }
            Err(FastlyStatus::BUFLEN) => Err(BufferSizeError::url(max_length, nwritten)),
            _ => panic!("fastly_http_req::uri_get failed"),
        }
    }

    pub(crate) fn get_url_impl(
        &self,
        mut initial_buf_size: usize,
        max_buf_size: Option<usize>,
    ) -> Result<Url, BufferSizeError> {
        if let Some(max) = max_buf_size {
            initial_buf_size = std::cmp::min(initial_buf_size, max);
        }
        match self.get_url(initial_buf_size) {
            Ok(url) => Ok(url),
            Err(mut err) => {
                if let Some(max) = max_buf_size {
                    // if there's a max size, enforce it
                    if err.needed_buf_size <= max {
                        self.get_url(err.needed_buf_size)
                    } else {
                        // report the maximum that was exceeded, not what we tried
                        err.buf_size = max;
                        Err(err)
                    }
                } else {
                    // otherwise just get as much as is needed
                    self.get_url(err.needed_buf_size)
                }
            }
        }
    }

    /// Set the request URL.
    pub fn set_url(&mut self, url: &Url) {
        let url_bytes = url.as_str().as_bytes();
        unsafe {
            abi::fastly_http_req::uri_set(self.as_u32(), url_bytes.as_ptr(), url_bytes.len())
        }
        .result()
        .expect("fastly_http_req::uri_set failed");
    }

    /// Send the request to the given backend server, and return once the response headers have been
    /// received, or an error occurs.
    pub fn send(
        self,
        body: BodyHandle,
        backend: &str,
    ) -> Result<(ResponseHandle, BodyHandle), SendErrorCause> {
        let mut resp_handle = ResponseHandle::INVALID;
        let mut resp_body_handle = BodyHandle::INVALID;
        let status = unsafe {
            abi::fastly_http_req::send(
                self.into_u32(),
                body.into_u32(),
                backend.as_ptr(),
                backend.len(),
                resp_handle.as_u32_mut(),
                resp_body_handle.as_u32_mut(),
            )
        };
        if status.is_err() {
            Err(SendErrorCause::status(status))
        } else if resp_handle.is_invalid() || resp_body_handle.is_invalid() {
            panic!("fastly_http_req::send returned invalid handles");
        } else {
            Ok((resp_handle, resp_body_handle))
        }
    }

    /// Send a request asynchronously via the given backend, returning as soon as the request has
    /// begun sending.
    ///
    /// The resulting [`PendingRequestHandle`] can be evaluated using
    /// [`PendingRequestHandle::poll()`], [`PendingRequestHandle::wait()`], or
    /// [`select_handles()`][`crate::handle::select_handles()`]. It can also be discarded if the
    /// request was sent for effects it might have, and the response is unimportant.
    pub fn send_async(
        self,
        body: BodyHandle,
        backend: &str,
    ) -> Result<PendingRequestHandle, SendErrorCause> {
        let mut pending_req_handle = PendingRequestHandle::INVALID;
        let status = unsafe {
            abi::fastly_http_req::send_async(
                self.into_u32(),
                body.into_u32(),
                backend.as_ptr(),
                backend.len(),
                pending_req_handle.as_u32_mut(),
            )
        };
        if status.is_err() {
            Err(SendErrorCause::status(status))
        } else if pending_req_handle.is_invalid() {
            panic!("fastly_http_req::send_async returned an invalid handle");
        } else {
            Ok(pending_req_handle)
        }
    }

    /// Send a request asynchronously via the given backend, and return a [`StreamingBodyHandle`] to
    /// allow continued writes to the request body.
    ///
    /// [`StreamingBodyHandle::finish()`] must be called in order to finish sending the request.
    pub fn send_async_streaming(
        self,
        body: BodyHandle,
        backend: &str,
    ) -> Result<(StreamingBodyHandle, PendingRequestHandle), SendErrorCause> {
        let mut pending_req_handle = PendingRequestHandle::INVALID;
        let status = unsafe {
            abi::fastly_http_req::send_async_streaming(
                self.into_u32(),
                body.as_u32(),
                backend.as_ptr(),
                backend.len(),
                pending_req_handle.as_u32_mut(),
            )
        };
        if status.is_err() {
            Err(SendErrorCause::status(status))
        } else if pending_req_handle.is_invalid() {
            panic!("fastly_http_req::send_async_streaming returned an invalid handle");
        } else {
            Ok((
                StreamingBodyHandle::from_body_handle(body),
                pending_req_handle,
            ))
        }
    }

    /// Set the cache override behavior for this request.
    ///
    /// This setting will override any cache directive headers returned in response to this request.
    pub fn set_cache_override(&mut self, cache_override: &CacheOverride) {
        let (tag, ttl, swr, sk) = cache_override.to_abi();
        let (sk_ptr, sk_len) = match sk {
            Some(sk) if sk.len() > 0 => (sk.as_ptr(), sk.len()),
            _ => (std::ptr::null(), 0),
        };

        unsafe {
            abi::fastly_http_req::cache_override_v2_set(
                self.as_u32(),
                tag,
                ttl,
                swr,
                sk_ptr,
                sk_len,
            )
        }
        .result()
        .expect("fastly_http_req::cache_override_v2_set failed");
    }

    /// Close the RequestHandle by removing it from the host Session. If the handle has already
    /// been closed an error will be returned. When calling
    /// send/send_async/send_async_streaming the RequestHandle is consumed and
    /// it's cleaned up. You should only call `close` if you have not sent a
    /// request yet and want to clean up the resources if not being used.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::handle::RequestHandle;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let request = RequestHandle::new();
    /// // The handle is not being used so we can close it out without any
    /// // trouble
    /// request.close()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn close(self) -> Result<(), HandleError> {
        match unsafe { abi::fastly_http_req::close(self.as_u32()) } {
            FastlyStatus::OK => Ok(()),
            _ => Err(HandleError::ClosedHandle(HandleKind::Request)),
        }
    }

    /// Set the content encodings to automatically decompress responses to this request.
    ///
    /// If the response to this request is encoded by one of the encodings set by this method, the
    /// response will be presented to the Compute@Edge program in decompressed form with the
    /// `Content-Encoding` and `Content-Length` headers removed.
    pub fn set_auto_decompress_response(&mut self, content_encodings: ContentEncodings) {
        unsafe {
            abi::fastly_http_req::auto_decompress_response_set(self.as_u32(), content_encodings)
        }
        .result()
        .expect("fastly_http_req::auto_decompress_response_set failed")
    }

    /// Sets the way that framing headers are determined for this request.
    pub fn set_framing_headers_mode(&mut self, mode: FramingHeadersMode) {
        unsafe { abi::fastly_http_req::framing_headers_mode_set(self.as_u32(), mode) }
            .result()
            .expect("fastly_http_req::framing_headers_mode_set failed")
    }
}

impl Drop for RequestHandle {
    fn drop(&mut self) {
        if self.is_valid() {
            unsafe { abi::fastly_http_req::close(self.as_u32()) }
                .result()
                .expect("fastly_http_req::close failed");
        }
    }
}

/// Get handles to the client request headers and body at the same time.
///
/// This will panic if either the parts of the body have already been retrieved.
pub fn client_request_and_body() -> (RequestHandle, BodyHandle) {
    RequestHandle::set_got_client();
    BodyHandle::set_got_client();
    let result = {
        let mut req_handle = RequestHandle::INVALID;
        let mut body_handle = BodyHandle::INVALID;
        let status = unsafe {
            abi::fastly_http_req::body_downstream_get(
                req_handle.as_u32_mut(),
                body_handle.as_u32_mut(),
            )
        };
        status.result().map(|_| (req_handle, body_handle))
    };
    match result {
        Ok((r, b)) if r.is_valid() && b.is_valid() => (r, b),
        _ => panic!("fastly_http_req::body_downstream_get failed"),
    }
}

/// Returns the client request's header names exactly as they were originally received.
///
/// This includes both the original header name characters' cases, as well as the original order of
/// the received headers.
///
/// If there is a header name that is longer than the provided buffer, this will return a
/// [`BufferSizeError`]; you can retry with a larger buffer size if necessary.
pub fn client_original_header_names(
    buf_size: usize,
) -> impl Iterator<Item = Result<String, BufferSizeError>> {
    client_original_header_names_impl(buf_size, Some(buf_size))
}

pub(crate) fn client_original_header_names_impl(
    mut initial_buf_size: usize,
    max_buf_size: Option<usize>,
) -> impl Iterator<Item = Result<String, BufferSizeError>> {
    if let Some(max) = max_buf_size {
        initial_buf_size = std::cmp::min(initial_buf_size, max);
    }
    abi::MultiValueHostcall::new(
        b'\0',
        initial_buf_size,
        max_buf_size,
        move |buf, buf_size, cursor, ending_cursor, nwritten| unsafe {
            abi::fastly_http_req::original_header_names_get(
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
            Ok(name_bytes) => Ok(String::from_utf8(name_bytes.to_vec()).unwrap()),
            // return an error if the buffer was not large enough
            Err(BufferTooSmall { needed_buf_size }) => Err(BufferSizeError::header_value(
                max_buf_size.expect("maximum buffer size must exist if a buffer size error occurs"),
                needed_buf_size,
            )),
            // panic if the hostcall failed for some other reason
            Err(ClosureError(e)) => {
                panic!("fastly_http_req::header_values_get returned error: {:?}", e)
            }
        }
    })
}

/// Returns the number of headers in the client request as originally received.
pub fn client_original_header_count() -> u32 {
    let mut count = 0;
    let status = unsafe { abi::fastly_http_req::original_header_count(&mut count) };
    if status.is_err() || count == 0 {
        panic!("downstream_original_header_count failed")
    }
    count
}

/// Returns whether or not a valid Fastly-Key for this service was received.
pub fn fastly_key_is_valid() -> bool {
    let mut is_valid = 0;
    let status = unsafe { abi::fastly_http_req::fastly_key_is_valid(&mut is_valid) };
    if status.is_err() {
        panic!("fastly_key_is_valid failed")
    }
    // Just in case more information needs to be conveyed by fastly_key_is_valid in the future, we
    // can at least establish that any authenticated key will have at least the lowest bit set.
    (is_valid & 1) != 0
}

/// Returns the IP address of the client making the HTTP request.
pub fn client_ip_addr() -> Option<IpAddr> {
    let mut octets = [0; 16];
    let mut nwritten = 0;

    let status = unsafe {
        abi::fastly_http_req::downstream_client_ip_addr(octets.as_mut_ptr(), &mut nwritten)
    };
    if status.is_err() {
        panic!("downstream_client_ip_addr failed");
    }
    match nwritten {
        4 => {
            let octets: [u8; 4] = octets[0..4]
                .try_into()
                .expect("octets is at least 4 bytes long");
            let addr: Ipv4Addr = octets.into();
            Some(addr.into())
        }
        16 => {
            let addr: Ipv6Addr = octets.into();
            Some(addr.into())
        }
        _ => panic!("downstream_client_ip_addr wrote an unexpected number of bytes"),
    }
}

pub fn redirect_to_websocket_proxy(backend: &str) -> FastlyStatus {
    unsafe { abi::fastly_http_req::redirect_to_websocket_proxy(backend.as_ptr(), backend.len()) }
}

pub fn redirect_to_grip_proxy(backend: &str) -> FastlyStatus {
    unsafe { abi::fastly_http_req::redirect_to_grip_proxy(backend.as_ptr(), backend.len()) }
}

/// Get the HTTP/2 fingerprint of client request if available
pub fn client_h2_fingerprint() -> Option<&'static str> {
    lazy_static! {
        static ref H2FP: Option<String> = {
            let name = "downstream HTTP/2 fingerprint";
            get_bytes_adaptive(
                abi::fastly_http_req::downstream_client_h2_fingerprint,
                512,
                name,
            )
            .map(|buf| {
                String::from_utf8(buf).unwrap_or_else(|_| panic!("{} must be valid UTF-8", name))
            })
        };
    }
    H2FP.as_ref().map(|x| x.as_str())
}

/// Get the id of the current equest if available
pub fn client_request_id() -> Option<&'static str> {
    lazy_static! {
        static ref REQID: Option<String> = {
            let name = "downstream request id";
            get_bytes_adaptive(
                abi::fastly_http_req::downstream_client_request_id,
                512,
                name,
            )
            .map(|buf| {
                String::from_utf8(buf).unwrap_or_else(|_| panic!("{} must be valid UTF-8", name))
            })
        };
    }
    REQID.as_ref().map(|x| x.as_str())
}

/// Get the raw bytes sent by the client in the TLS ClientHello message.
///
/// See [RFC 5246](https://tools.ietf.org/html/rfc5246#section-7.4.1.2) for details.
pub fn client_tls_client_hello() -> Option<&'static [u8]> {
    lazy_static! {
        static ref CLIENT_HELLO: Option<Vec<u8>> = {
            get_bytes_adaptive(
                abi::fastly_http_req::downstream_tls_client_hello,
                512,
                "downstream TLS ClientHello",
            )
        };
    }
    CLIENT_HELLO.as_ref().map(|x| x.as_ref())
}

/// Get the JA3 hash of the TLS ClientHello message.
pub fn client_tls_ja3_md5() -> Option<[u8; 16]> {
    let mut ja3_md5 = [0; 16];
    let mut nwritten = 0;

    let status = unsafe {
        abi::fastly_http_req::downstream_tls_ja3_md5(ja3_md5.as_mut_ptr(), &mut nwritten)
    };
    if status.is_err() {
        panic!("downstream_tls_ja3_md5 failed");
    }
    match nwritten {
        16 => Some(ja3_md5),
        _ => panic!("downstream_tls_ja3_md5 wrote an unexpected number of bytes"),
    }
}

/// Get the client certificate used to secure the downstream client mTLS connection.
///
/// The value returned will be based on PEM format.
pub fn client_tls_client_raw_certificate() -> Option<&'static str> {
    lazy_static! {
        static ref CLIENT_CERTIFICATE: Option<String> = {
            let name = "downstream TLS client raw certificate";
            get_bytes_adaptive(
                abi::fastly_http_req::downstream_tls_raw_client_certificate,
                4096,
                name,
            )
            .map_or(Some(String::from("")), |buf| {
                Some(
                    String::from_utf8(buf)
                        .unwrap_or_else(|_| panic!("{} must be valid UTF-8", name)),
                )
            })
        };
    }
    CLIENT_CERTIFICATE.as_ref().map(|x| x.as_ref())
}

/// Returns the [`ClientCertVerifyResult`] from the downstream client mTLS handshake.
///
/// Returns `None` if not available.
pub fn client_tls_client_cert_verify_result() -> Option<ClientCertVerifyResult> {
    let mut raw_verify_result = 0;

    let status = unsafe {
        abi::fastly_http_req::downstream_tls_client_cert_verify_result(&mut raw_verify_result)
    };
    if status.is_err() {
        return None;
    }

    let verify_result = ClientCertVerifyResult::from_u32(raw_verify_result);
    Some(verify_result)
}

/// Get the cipher suite used to secure the downstream client TLS connection.
///
/// The value returned will be consistent with the [OpenSSL
/// name](https://testssl.sh/openssl-iana.mapping.html) for the cipher suite.
///
/// # Examples
///
/// ```no_run
/// assert_eq!(
///     fastly::handle::client_tls_cipher_openssl_name().unwrap(),
///     "ECDHE-RSA-AES128-GCM-SHA256"
/// );
/// ```
pub fn client_tls_cipher_openssl_name() -> Option<&'static str> {
    lazy_static! {
        static ref OPENSSL_NAME: Option<String> = {
            let name = "downstream TLS cipher OpenSSL name";
            get_bytes_adaptive(
                abi::fastly_http_req::downstream_tls_cipher_openssl_name,
                128,
                name,
            )
            .map(|buf| {
                String::from_utf8(buf).unwrap_or_else(|_| panic!("{} must be valid UTF-8", name))
            })
        };
    }
    OPENSSL_NAME.as_ref().map(|x| x.as_ref())
}

/// Get the TLS protocol version used to secure the downstream client TLS connection.
///
/// # Examples
///
/// ```no_run
/// # use fastly::Request;
/// assert_eq!(Request::from_client().get_tls_protocol().unwrap(), "TLSv1.2");
/// ```
pub fn client_tls_protocol() -> Option<&'static str> {
    lazy_static! {
        static ref PROTOCOL: Option<String> = {
            let name = "downstream TLS cipher protocol";
            get_bytes_adaptive(abi::fastly_http_req::downstream_tls_protocol, 32, name).map(|buf| {
                String::from_utf8(buf).unwrap_or_else(|_| panic!("{} must be valid UTF-8", name))
            })
        };
    }
    PROTOCOL.as_ref().map(|x| x.as_str())
}

fn get_bytes_adaptive(
    hostcall: unsafe extern "C" fn(*mut u8, usize, *mut usize) -> FastlyStatus,
    default_buf_size: usize,
    name: &str,
) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(default_buf_size);
    let mut nwritten = 0;

    let status = unsafe { hostcall(buf.as_mut_ptr(), buf.capacity(), &mut nwritten) };

    match status {
        FastlyStatus::OK => (),
        FastlyStatus::BUFLEN if nwritten != 0 => {
            buf.reserve_exact(nwritten);
            let status = unsafe { hostcall(buf.as_mut_ptr(), buf.capacity(), &mut nwritten) };
            if status.is_err() {
                panic!("couldn't get the {}", name);
            }
        }
        FastlyStatus::ERROR => {
            // ERROR can indicate that TLS metadata simply isn't present. This is the case when the
            // client request is non-TLS.
            return None;
        }
        _ => panic!("couldn't get the {}", name),
    };

    unsafe {
        buf.set_len(nwritten);
    }
    Some(buf)
}
