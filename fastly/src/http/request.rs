//! HTTP requests.

use self::handle::{ContentEncodings, RequestHandle};
use super::body::{self, Body, StreamingBody};
use super::response::{handles_to_response, FastlyResponseMetadata, Response};
use crate::convert::{Borrowable, ToBackend, ToHeaderName, ToHeaderValue, ToMethod, ToUrl};
use crate::error::{ensure, BufferSizeError, Error};
use crate::handle::BodyHandle;
use crate::limits::{self, RequestLimits};
use fastly_shared::{CacheOverride, ClientCertVerifyResult, FramingHeadersMode};
use http::header::{HeaderName, HeaderValue};
use http::{HeaderMap, Method, Version};
use mime::Mime;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::borrow::Cow;
use std::fmt;
use std::io::BufRead;
use std::net::IpAddr;
use std::sync::Arc;
use thiserror::Error;
use url::Url;

pub use pending::{select, PendingRequest, PollResult};

#[macro_use]
mod macros;

pub(crate) mod handle;
pub(crate) mod pending;

/// An HTTP request, including body, headers, method, and URL.
///
/// # Getting the client request
///
/// Call [`Request::from_client()`] to get the client request being handled by this execution of the
/// Compute@Edge program.
///
/// # Creation and conversion
///
/// New requests can be created programmatically with [`Request::new()`]. In addition, there are
/// convenience constructors like [`Request::get()`] which automatically select the appropriate
/// method.
///
/// For interoperability with other Rust libraries, [`Request`] can be converted to and from the
/// [`http`] crate's [`http::Request`] type using the [`From`][`Self::from()`] and
/// [`Into`][`Self::into()`] traits.
///
/// # Sending backend requests
///
/// Requests can be sent to a backend in blocking or asynchronous fashion using
/// [`send()`][`Self::send()`], [`send_async()`][`Self::send_async()`], or
/// [`send_async_streaming()`][`Self::send_async_streaming()`].
///
/// # Builder-style methods
///
/// [`Request`] can be used as a
/// [builder](https://doc.rust-lang.org/1.0.0/style/ownership/builders.html), allowing requests to
/// be constructed and used through method chaining. Methods with the `with_` name prefix, such as
/// [`with_header()`][`Self::with_header()`], return `Self` to allow chaining. The builder style is
/// typically most useful when constructing and using a request in a single expression. For example:
///
/// ```no_run
/// # use fastly::{Error, Request};
/// # fn f() -> Result<(), Error> {
/// Request::get("https://example.com")
///     .with_header("my-header", "hello!")
///     .with_header("my-other-header", "Здравствуйте!")
///     .send("example_backend")?;
/// # Ok(()) }
/// ```
///
/// # Setter methods
///
/// Setter methods, such as [`set_header()`][`Self::set_header()`], are prefixed by `set_`, and can
/// be used interchangeably with the builder-style methods, allowing you to mix and match styles
/// based on what is most convenient for your program. Setter methods tend to work better than
/// builder-style methods when constructing a request involves conditional branches or loops. For
/// example:
///
/// ```no_run
/// # use fastly::{Error, Request};
/// # fn f(needs_translation: bool) -> Result<(), Error> {
/// let mut req = Request::get("https://example.com").with_header("my-header", "hello!");
/// if needs_translation {
///     req.set_header("my-other-header", "Здравствуйте!");
/// }
/// req.send("example_backend")?;
/// # Ok(()) }
/// ```
#[derive(Debug)]
pub struct Request {
    version: Version,
    method: Method,
    url: Url,
    headers: HeaderMap,
    body: Option<Body>,
    cache_override: CacheOverride,
    is_from_client: bool,
    auto_decompress_response: ContentEncodings,
    framing_headers_mode: FramingHeadersMode,
    // Overridden via experimental::RequestCacheKey
    pub(crate) cache_key: Option<CacheKeyGen>,
}

#[derive(Clone)]
pub(crate) enum CacheKeyGen {
    Lazy(Arc<dyn Fn(&Request) -> [u8; 32] + Send + Sync>),
    Set([u8; 32]),
}

impl std::fmt::Debug for CacheKeyGen {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            CacheKeyGen::Lazy(f) => fmt.write_fmt(format_args!("Lazy({:?})", Arc::as_ptr(f))),
            CacheKeyGen::Set(k) => {
                const DIGITS: &[u8] = b"0123456789ABCDEF";
                let mut hex = [0; 64];
                for (i, b) in k.iter().enumerate() {
                    hex[i * 2] = DIGITS[(b >> 4) as usize];
                    hex[i * 2 + 1] = DIGITS[(b & 0xf) as usize];
                }
                fmt.write_fmt(format_args!("Set({})", std::str::from_utf8(&hex).unwrap()))
            }
        }
    }
}

impl Request {
    /// Get the client request being handled by this execution of the Compute@Edge program.
    ///
    /// # Panics
    ///
    /// This method panics if the client request has already been retrieved by this method,
    /// [`Request::try_from_client()`], or by [the low-level handle API][`crate::handle`].
    ///
    /// If the request exceeds the limits specified by [`RequestLimits`], this method sends an empty
    /// response with a `400 BAD REQUEST` HTTP status to the client, and then panics. Use
    /// [`try_from_client()`][`Self::try_from_client()`] if you want to explicitly handle these
    /// errors, for example by returning a customized error page.
    ///
    /// # Incompatibility with [`fastly::main`][`crate::main`]
    ///
    /// This method cannot be used with [`fastly::main`][`crate::main`], as that attribute
    /// implicitly calls [`Request::from_client()`] to populate the request argument. Use an
    /// undecorated `main()` function instead, along with [`Response::send_to_client()`] or
    /// [`Response::stream_to_client()`] to send a response to the client.
    pub fn from_client() -> Request {
        Request::try_from_client().unwrap_or_else(|e| {
            panic_with_status!(
                crate::http::StatusCode::BAD_REQUEST,
                "fastly::limits::RequestLimits exceeded: {}",
                e
            )
        })
    }

    /// Get the client request being handled by this execution of the Compute@Edge program, or an
    /// error if the request exceeds the limits specified by [`RequestLimits`].
    ///
    /// # Panics
    ///
    /// This method panics if the client request has already been retrieved by this method,
    /// [`Request::from_client()`], or by [the low-level handle API][`crate::handle`].
    pub fn try_from_client() -> Result<Request, BufferSizeError> {
        let (req_handle, body_handle) = self::handle::client_request_and_body();
        Request::from_handles(req_handle, Some(body_handle))
    }

    /// Return `true` if this request is from the client of this execution of the Compute@Edge
    /// program.
    pub fn is_from_client(&self) -> bool {
        self.is_from_client
    }

    /// Create a new request with the given method and URL, no headers, and an empty body.
    ///
    /// # Argument type conversion
    ///
    /// The method and URL arguments can be any types that implement [`ToMethod`] and [`ToUrl`],
    /// respectively. See those traits for details on which types can be used and when panics may
    /// arise during conversion.
    pub fn new(method: impl ToMethod, url: impl ToUrl) -> Self {
        Self {
            version: Version::HTTP_11,
            method: method.into_owned(),
            url: url.into_owned(),
            headers: HeaderMap::new(),
            body: None,
            cache_override: CacheOverride::default(),
            is_from_client: false,
            auto_decompress_response: ContentEncodings::empty(),
            framing_headers_mode: FramingHeadersMode::Automatic,
            cache_key: None,
        }
    }

    /// Make a new request with the same method, url, headers, and version of this request, but no
    /// body.
    ///
    /// If you also need to clone the request body, use
    /// [`clone_with_body()`][`Self::clone_with_body()`]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let original = Request::post("https://example.com")
    ///     .with_header("hello", "world!")
    ///     .with_body("hello");
    /// let new = original.clone_without_body();
    /// assert_eq!(original.get_method(), new.get_method());
    /// assert_eq!(original.get_url(), new.get_url());
    /// assert_eq!(original.get_header("hello"), new.get_header("hello"));
    /// assert_eq!(original.get_version(), new.get_version());
    /// assert!(original.has_body());
    /// assert!(!new.has_body());
    /// ```
    pub fn clone_without_body(&self) -> Request {
        Self {
            version: self.version,
            method: self.method.clone(),
            url: self.url.clone(),
            headers: self.headers.clone(),
            body: None,
            cache_override: self.cache_override.clone(),
            is_from_client: self.is_from_client,
            auto_decompress_response: self.auto_decompress_response,
            framing_headers_mode: self.framing_headers_mode,
            cache_key: self.cache_key.clone(),
        }
    }

    /// Clone this request by reading in its body, and then writing the same body to the original
    /// and the cloned request.
    ///
    /// This method requires mutable access to this request because reading from and writing to the
    /// body can involve an HTTP connection.
    ///
    #[doc = include_str!("../../docs/snippets/clones-body.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut original = Request::post("https://example.com")
    ///     .with_header("hello", "world!")
    ///     .with_body("hello");
    /// let mut new = original.clone_with_body();
    /// assert_eq!(original.get_method(), new.get_method());
    /// assert_eq!(original.get_url(), new.get_url());
    /// assert_eq!(original.get_header("hello"), new.get_header("hello"));
    /// assert_eq!(original.get_version(), new.get_version());
    /// assert_eq!(original.take_body_bytes(), new.take_body_bytes());
    /// ```
    pub fn clone_with_body(&mut self) -> Request {
        let mut new_req = self.clone_without_body();
        if self.has_body() {
            for chunk in self.take_body().read_chunks(4096) {
                let chunk = chunk.expect("can read body chunk");
                new_req.get_body_mut().write_bytes(&chunk);
                self.get_body_mut().write_bytes(&chunk);
            }
        }
        new_req
    }

    /// Create a new `GET` [`Request`] with the given URL, no headers, and an empty body.
    ///
    #[doc = include_str!("../../docs/snippets/url-argument.md")]
    pub fn get(url: impl ToUrl) -> Self {
        Self::new(Method::GET, url)
    }

    /// Create a new `HEAD` [`Request`] with the given URL, no headers, and an empty body.
    ///
    #[doc = include_str!("../../docs/snippets/url-argument.md")]
    pub fn head(url: impl ToUrl) -> Self {
        Self::new(Method::HEAD, url)
    }

    /// Create a new `POST` [`Request`] with the given URL, no headers, and an empty body.
    ///
    #[doc = include_str!("../../docs/snippets/url-argument.md")]
    pub fn post(url: impl ToUrl) -> Self {
        Self::new(Method::POST, url)
    }

    /// Create a new `PUT` [`Request`] with the given URL, no headers, and an empty body.
    ///
    #[doc = include_str!("../../docs/snippets/url-argument.md")]
    pub fn put(url: impl ToUrl) -> Self {
        Self::new(Method::PUT, url)
    }

    /// Create a new `DELETE` [`Request`] with the given URL, no headers, and an empty body.
    ///
    #[doc = include_str!("../../docs/snippets/url-argument.md")]
    pub fn delete(url: impl ToUrl) -> Self {
        Self::new(Method::DELETE, url)
    }

    /// Create a new `CONNECT` [`Request`] with the given URL, no headers, and an empty body.
    ///
    #[doc = include_str!("../../docs/snippets/url-argument.md")]
    pub fn connect(url: impl ToUrl) -> Self {
        Self::new(Method::CONNECT, url)
    }

    /// Create a new `OPTIONS` [`Request`] with the given URL, no headers, and an empty body.
    ///
    #[doc = include_str!("../../docs/snippets/url-argument.md")]
    pub fn options(url: impl ToUrl) -> Self {
        Self::new(Method::OPTIONS, url)
    }

    /// Create a new `TRACE` [`Request`] with the given URL, no headers, and an empty body.
    ///
    #[doc = include_str!("../../docs/snippets/url-argument.md")]
    pub fn trace(url: impl ToUrl) -> Self {
        Self::new(Method::TRACE, url)
    }

    /// Create a new `PATCH` [`Request`] with the given URL, no headers, and an empty body.
    ///
    #[doc = include_str!("../../docs/snippets/url-argument.md")]
    pub fn patch(url: impl ToUrl) -> Self {
        Self::new(Method::PATCH, url)
    }

    /// Send the request to the given backend server, and return once the response headers have been
    /// received, or an error occurs.
    ///
    #[doc = include_str!("../../docs/snippets/backend-argument.md")]
    ///
    /// # Panics
    ///
    #[doc = include_str!("../../docs/snippets/panics-responselimits.md")]
    ///
    /// # Examples
    ///
    /// Sending the client request to a backend without modification:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let backend_resp = Request::from_client().send("example_backend").expect("request succeeds");
    /// assert!(backend_resp.get_status().is_success());
    /// ```
    ///
    /// Sending a synthetic request:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let backend_resp = Request::get("https://example.com")
    ///     .send("example_backend")
    ///     .expect("request succeeds");
    /// assert!(backend_resp.get_status().is_success());
    /// ```
    pub fn send(self, backend: impl ToBackend) -> Result<Response, SendError> {
        let backend = backend.into_owned();
        let (req_handle, req_body_handle, sent_req) = self.prepare_handles(&backend)?;
        let (resp_handle, resp_body_handle) = try_with_req!(
            backend.name(),
            sent_req,
            req_handle.send(req_body_handle, backend.name())
        );
        handles_to_response(
            resp_handle,
            resp_body_handle,
            FastlyResponseMetadata::new(backend, sent_req),
        )
    }

    /// Begin sending the request to the given backend server, and return a [`PendingRequest`] that
    /// can yield the backend response or an error.
    ///
    /// This method returns as soon as the request begins sending to the backend, and transmission
    /// of the request body and headers will continue in the background.
    ///
    /// This method allows for sending more than one request at once and receiving their responses
    /// in arbitrary orders. See [`PendingRequest`] for more details on how to wait on, poll, or
    /// select between pending requests.
    ///
    /// This method is also useful for sending requests where the response is unimportant, but the
    /// request may take longer than the Compute@Edge program is able to run, as the request will
    /// continue sending even after the program that initiated it exits.
    ///
    #[doc = include_str!("../../docs/snippets/backend-argument.md")]
    ///
    /// # Examples
    ///
    /// Sending a request to two backends and returning whichever response finishes first:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let backend_resp_1 = Request::get("https://example.com/")
    ///     .send_async("example_backend_1")
    ///     .expect("request 1 begins sending");
    /// let backend_resp_2 = Request::get("https://example.com/")
    ///     .send_async("example_backend_2")
    ///     .expect("request 2 begins sending");
    /// let (resp, _) = fastly::http::request::select(vec![backend_resp_1, backend_resp_2]);
    /// resp.expect("request succeeds").send_to_client();
    /// ```
    ///
    /// Sending a long-running request and ignoring its result so that the program can exit before
    /// it completes:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// # let some_large_file = vec![0u8];
    /// let _ = Request::post("https://example.com")
    ///     .with_body(some_large_file)
    ///     .send_async("example_backend");
    /// ```
    pub fn send_async(self, backend: impl ToBackend) -> Result<PendingRequest, SendError> {
        let backend = backend.into_owned();
        let (req_handle, req_body_handle, sent_req) = self.prepare_handles(&backend)?;
        let pending_req_handle = try_with_req!(
            backend.name(),
            sent_req,
            req_handle.send_async(req_body_handle, backend.name())
        );
        let pending_req = PendingRequest::new(
            pending_req_handle,
            FastlyResponseMetadata::new(backend, sent_req),
        );
        Ok(pending_req)
    }

    /// Begin sending the request to the given backend server, and return a [`PendingRequest`] that
    /// can yield the backend response or an error along with a [`StreamingBody`] that can accept
    /// further data to send.
    ///
    /// The backend connection is only closed once [`StreamingBody::finish()`] is called. The
    /// [`PendingRequest`] will not yield a [`Response`] until the [`StreamingBody`] is finished.
    ///
    /// This method is most useful for programs that do some sort of processing or inspection of a
    /// potentially-large client request body. Streaming allows the program to operate on small
    /// parts of the body rather than having to read it all into memory at once.
    ///
    /// This method returns as soon as the request begins sending to the backend, and transmission
    /// of the request body and headers will continue in the background.
    ///
    /// # Examples
    ///
    /// Count the number of lines in a UTF-8 client request body while sending it to the backend:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// use std::io::BufRead;
    ///
    /// let mut req = Request::from_client();
    /// // Take the body so we can iterate through its lines later
    /// let req_body = req.take_body();
    /// // Start sending the client request to the client with a now-empty body
    /// let (mut backend_body, pending_req) = req
    ///     .send_async_streaming("example_backend")
    ///     .expect("request begins sending");
    ///
    /// let mut num_lines = 0;
    /// for line in req_body.lines() {
    ///     let line = line.unwrap();
    ///     num_lines += 1;
    ///     // Write the line to the streaming backend body
    ///     backend_body.write_str(&line);
    /// }
    /// // Finish the streaming body to allow the backend connection to close
    /// backend_body.finish().unwrap();
    ///
    /// println!("client request body contained {} lines", num_lines);
    /// ```
    pub fn send_async_streaming(
        self,
        backend: impl ToBackend,
    ) -> Result<(StreamingBody, PendingRequest), SendError> {
        let backend = backend.into_owned();
        let (req_handle, req_body_handle, sent_req) = self.prepare_handles(&backend)?;
        let (streaming_body_handle, pending_req_handle) = try_with_req!(
            backend.name(),
            sent_req,
            req_handle.send_async_streaming(req_body_handle, backend.name())
        );
        let pending_req = PendingRequest::new(
            pending_req_handle,
            FastlyResponseMetadata::new(backend, sent_req),
        );
        Ok((streaming_body_handle.into(), pending_req))
    }

    /// A helper function for decomposing a [`Request`] into handles for use in hostcalls.
    ///
    /// Note that in addition to the [`RequestHandle`] and [`BodyHandle`], the tuple returned also
    /// includes a copy of the original request so that metadata about the request can be inspected
    /// later using the `FastlyResponseMetadata` extension.
    ///
    /// This will return an error if the backend name is invalid, or if the request does not have a
    /// valid URI.
    fn prepare_handles(
        mut self,
        backend: impl ToBackend,
    ) -> Result<(RequestHandle, BodyHandle, Self), SendError> {
        // First, validate the request.
        if let Err(e) = validate_request(&self) {
            return Err(SendError::new(
                backend.into_borrowable().as_ref().name(),
                self,
                SendErrorCause::Generic(e),
            ));
        }
        let (req_handle, body_handle) = self.to_handles();
        Ok((
            req_handle,
            // TODO ACF 2020-11-30: it'd be nice to change the ABI so that body handles were
            // optional to save a hostcall for many requests
            body_handle.unwrap_or_else(|| BodyHandle::new()),
            self,
        ))
    }

    /// Builder-style equivalent of [`set_body()`][`Self::set_body()`].
    pub fn with_body(mut self, body: impl Into<Body>) -> Self {
        self.set_body(body);
        self
    }

    /// Returns `true` if this request has a body.
    pub fn has_body(&self) -> bool {
        self.body.is_some()
    }

    /// Get a mutable reference to the body of this request.
    ///
    #[doc = include_str!("../../docs/snippets/creates-empty-body.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// use std::io::Write;
    ///
    /// let mut req = Request::post("https://example.com").with_body("hello,");
    /// write!(req.get_body_mut(), " world!").unwrap();
    /// assert_eq!(&req.into_body_str(), "hello, world!");
    /// ```
    pub fn get_body_mut(&mut self) -> &mut Body {
        self.body.get_or_insert_with(|| Body::new())
    }

    /// Get a shared reference to the body of this request if it has one, otherwise return `None`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// use std::io::Write;
    ///
    /// let mut req = Request::post("https://example.com");
    /// assert!(req.try_get_body_mut().is_none());
    ///
    /// req.set_body("hello,");
    /// write!(req.try_get_body_mut().expect("body now exists"), " world!").unwrap();
    /// assert_eq!(&req.into_body_str(), "hello, world!");
    /// ```
    pub fn try_get_body_mut(&mut self) -> Option<&mut Body> {
        self.body.as_mut()
    }

    /// Get a prefix of this request's body containing up to the given number of bytes.
    ///
    /// See [`Body::get_prefix_mut()`] for details.
    pub fn get_body_prefix_mut(&mut self, length: usize) -> body::Prefix {
        self.get_body_mut().get_prefix_mut(length)
    }

    /// Get a prefix of this request's body as a string containing up to the given number of bytes.
    ///
    /// See [`Body::get_prefix_str_mut()`] for details.
    ///
    /// # Panics
    ///
    /// If the prefix contains invalid UTF-8 bytes, this function will panic. The exception to this
    /// is if the bytes are invalid because a multi-byte codepoint is cut off by the requested
    /// prefix length. In this case, the invalid bytes are left off the end of the prefix.
    ///
    /// To explicitly handle the possibility of invalid UTF-8 bytes, use
    /// [`try_get_body_prefix_str_mut()`][`Self::try_get_body_prefix_str_mut()`], which returns an
    /// error on failure rather than panicking.
    pub fn get_body_prefix_str_mut(&mut self, length: usize) -> body::PrefixString {
        self.get_body_mut().get_prefix_str_mut(length)
    }

    /// Try to get a prefix of the body as a string containing up to the given number of bytes.
    ///
    /// See [`Body::try_get_prefix_str_mut()`] for details.
    pub fn try_get_body_prefix_str_mut(
        &mut self,
        length: usize,
    ) -> Result<body::PrefixString, std::str::Utf8Error> {
        self.get_body_mut().try_get_prefix_str_mut(length)
    }

    /// Set the given value as the request's body.
    #[doc = include_str!("../../docs/snippets/body-argument.md")]
    #[doc = include_str!("../../docs/snippets/discards-body.md")]
    pub fn set_body(&mut self, body: impl Into<Body>) {
        self.body = Some(body.into());
    }

    /// Take and return the body from this request.
    ///
    /// After calling this method, this request will no longer have a body.
    ///
    #[doc = include_str!("../../docs/snippets/creates-empty-body.md")]
    pub fn take_body(&mut self) -> Body {
        self.body.take().unwrap_or_else(|| Body::new())
    }

    /// Take and return the body from this request if it has one, otherwise return `None`.
    ///
    /// After calling this method, this request will no longer have a body.
    pub fn try_take_body(&mut self) -> Option<Body> {
        self.body.take()
    }

    /// Append another [`Body`] to the body of this request without reading or writing any body
    /// contents.
    ///
    /// If this request does not have a body, the appended body is set as the request's body.
    ///
    #[doc = include_str!("../../docs/snippets/body-append-constant-time.md")]
    ///
    /// This method should be used when combining bodies that have not necessarily been read yet,
    /// such as the body of the client. To append contents that are already in memory as strings or
    /// bytes, you should instead use [`get_body_mut()`][`Self::get_body_mut()`] to write the
    /// contents to the end of the body.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::post("https://example.com").with_body("hello! client says: ");
    /// req.append_body(Request::from_client().into_body());
    /// req.send("example_backend").unwrap();
    /// ```
    pub fn append_body(&mut self, other: Body) {
        if let Some(ref mut body) = &mut self.body {
            body.append(other);
        } else {
            self.body = Some(other);
        }
    }

    /// Consume the request and return its body as a byte vector.
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body-reqresp.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::post("https://example.com").with_body(b"hello, world!".to_vec());
    /// let bytes = req.into_body_bytes();
    /// assert_eq!(&bytes, b"hello, world!");
    pub fn into_body_bytes(mut self) -> Vec<u8> {
        self.take_body_bytes()
    }

    /// Consume the request and return its body as a string.
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body-reqresp.md")]
    ///
    /// # Panics
    ///
    #[doc = include_str!("../../docs/snippets/panics-reqresp-intobody-utf8.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::post("https://example.com").with_body("hello, world!");
    /// let string = req.into_body_str();
    /// assert_eq!(&string, "hello, world!");
    /// ```
    pub fn into_body_str(mut self) -> String {
        self.take_body_str()
    }

    /// Consume the request and return its body as a string, including invalid characters.
    ///
    #[doc = include_str!("../../docs/snippets/utf8-replacement.md")]
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body-reqresp.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::post("https://example.com");
    /// req.set_body_octet_stream(b"\xF0\x90\x80 hello, world!");
    /// let string = req.into_body_str_lossy();
    /// assert_eq!(&string, "� hello, world!");
    /// ```
    pub fn into_body_str_lossy(mut self) -> String {
        self.take_body_str_lossy()
    }

    /// Consume the request and return its body.
    ///
    #[doc = include_str!("../../docs/snippets/creates-empty-body.md")]
    pub fn into_body(self) -> Body {
        self.body.unwrap_or_else(|| Body::new())
    }

    /// Consume the request and return its body if it has one, otherwise return `None`.
    pub fn try_into_body(self) -> Option<Body> {
        self.body
    }

    /// Builder-style equivalent of [`set_body_text_plain()`][`Self::set_body_text_plain()`].
    pub fn with_body_text_plain(mut self, body: &str) -> Self {
        self.set_body_text_plain(body);
        self
    }

    /// Set the given string as the request's body with content type `text/plain; charset=UTF-8`.
    ///
    #[doc = include_str!("../../docs/snippets/discards-body.md")]
    #[doc = include_str!("../../docs/snippets/sets-text-plain.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::post("https://example.com");
    /// req.set_body_text_plain("hello, world!");
    /// assert_eq!(req.get_content_type(), Some(fastly::mime::TEXT_PLAIN_UTF_8));
    /// assert_eq!(&req.into_body_str(), "hello, world!");
    /// ```
    pub fn set_body_text_plain(&mut self, body: &str) {
        self.body = Some(Body::from(body));
        self.set_content_type(mime::TEXT_PLAIN_UTF_8);
    }

    /// Builder-style equivalent of [`set_body_text_html()`][`Self::set_body_text_html()`].
    pub fn with_body_text_html(mut self, body: &str) -> Self {
        self.set_body_text_html(body);
        self
    }

    /// Set the given string as the request's body with content type `text/html; charset=UTF-8`.
    ///
    #[doc = include_str!("../../docs/snippets/discards-body.md")]
    #[doc = include_str!("../../docs/snippets/sets-text-html.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::post("https://example.com");
    /// req.set_body_text_html("<p>hello, world!</p>");
    /// assert_eq!(req.get_content_type(), Some(fastly::mime::TEXT_HTML_UTF_8));
    /// assert_eq!(&req.into_body_str(), "<p>hello, world!</p>");
    /// ```
    pub fn set_body_text_html(&mut self, body: &str) {
        self.body = Some(Body::from(body));
        self.set_content_type(mime::TEXT_HTML_UTF_8);
    }

    /// Take and return the body from this request as a string.
    ///
    /// After calling this method, this request will no longer have a body.
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body-reqresp.md")]
    ///
    /// # Panics
    ///
    #[doc = include_str!("../../docs/snippets/panics-reqresp-takebody-utf8.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::post("https://example.com").with_body("hello, world!");
    /// let string = req.take_body_str();
    /// assert!(req.try_take_body().is_none());
    /// assert_eq!(&string, "hello, world!");
    /// ```
    pub fn take_body_str(&mut self) -> String {
        if let Some(body) = self.try_take_body() {
            body.into_string()
        } else {
            String::new()
        }
    }

    /// Take and return the body from this request as a string, including invalid characters.
    ///
    #[doc = include_str!("../../docs/snippets/utf8-replacement.md")]
    ///
    /// After calling this method, this request will no longer have a body.
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body-reqresp.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::post("https://example.com");
    /// req.set_body_octet_stream(b"\xF0\x90\x80 hello, world!");
    /// let string = req.take_body_str_lossy();
    /// assert!(req.try_take_body().is_none());
    /// assert_eq!(&string, "� hello, world!");
    /// ```
    pub fn take_body_str_lossy(&mut self) -> String {
        if let Some(body) = self.try_take_body() {
            String::from_utf8_lossy(&body.into_bytes()).to_string()
        } else {
            String::new()
        }
    }

    /// Return a [`Lines`][`std::io::Lines`] iterator that reads the request body a line at a time.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::{Body, Request};
    /// use std::io::Write;
    ///
    /// fn remove_es(req: &mut Request) {
    ///     let mut no_es = Body::new();
    ///     for line in req.read_body_lines() {
    ///         writeln!(no_es, "{}", line.unwrap().replace("e", "")).unwrap();
    ///     }
    ///     req.set_body(no_es);
    /// }
    /// ```
    pub fn read_body_lines(&mut self) -> std::io::Lines<&mut Body> {
        self.get_body_mut().lines()
    }

    /// Builder-style equivalent of [`set_body_octet_stream()`][`Self::set_body_octet_stream()`].
    pub fn with_body_octet_stream(mut self, body: &[u8]) -> Self {
        self.set_body_octet_stream(body);
        self
    }

    /// Set the given bytes as the request's body.
    ///
    #[doc = include_str!("../../docs/snippets/discards-body.md")]
    #[doc = include_str!("../../docs/snippets/sets-app-octet-stream.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::post("https://example.com");
    /// req.set_body_octet_stream(b"hello, world!");
    /// assert_eq!(req.get_content_type(), Some(fastly::mime::APPLICATION_OCTET_STREAM));
    /// assert_eq!(&req.into_body_bytes(), b"hello, world!");
    /// ```
    pub fn set_body_octet_stream(&mut self, body: &[u8]) {
        self.body = Some(Body::from(body));
        self.set_content_type(mime::APPLICATION_OCTET_STREAM);
    }

    /// Take and return the body from this request as a string.
    ///
    /// After calling this method, this request will no longer have a body.
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body-reqresp.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::post("https://example.com").with_body(b"hello, world!".to_vec());
    /// let bytes = req.take_body_bytes();
    /// assert!(req.try_take_body().is_none());
    /// assert_eq!(&bytes, b"hello, world!");
    /// ```
    pub fn take_body_bytes(&mut self) -> Vec<u8> {
        if let Some(body) = self.try_take_body() {
            body.into_bytes()
        } else {
            Vec::new()
        }
    }

    /// Return an iterator that reads the request body in chunks of at most the given number of
    /// bytes.
    ///
    /// If `chunk_size` does not evenly divide the length of the body, then the last chunk will not
    /// have length `chunk_size`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::{Body, Request};
    /// fn remove_0s(req: &mut Request) {
    ///     let mut no_0s = Body::new();
    ///     for chunk in req.read_body_chunks(4096) {
    ///         let mut chunk = chunk.unwrap();
    ///         chunk.retain(|b| *b != 0);
    ///         no_0s.write_bytes(&chunk);
    ///     }
    ///     req.set_body(no_0s);
    /// }
    /// ```
    pub fn read_body_chunks<'a>(
        &'a mut self,
        chunk_size: usize,
    ) -> impl Iterator<Item = Result<Vec<u8>, std::io::Error>> + 'a {
        self.get_body_mut().read_chunks(chunk_size)
    }

    /// Builder-style equivalent of [`set_body_json()`][Self::set_body_json()`].
    pub fn with_body_json(mut self, value: &impl Serialize) -> Result<Self, serde_json::Error> {
        self.set_body_json(value)?;
        Ok(self)
    }

    /// Convert the given value to JSON and set that JSON as the request's body.
    ///
    /// The given value must implement [`serde::Serialize`]. You can either implement that trait for
    /// your own custom type, or use [`serde_json::Value`] to create untyped JSON values. See
    /// [`serde_json`] for details.
    ///
    #[doc = include_str!("../../docs/snippets/discards-body.md")]
    ///
    /// # Content type
    ///
    /// This method sets the content type to `application/json`.
    ///
    /// # Errors
    ///
    /// This method returns [`serde_json::Error`] if serialization fails.
    ///
    /// # Examples
    ///
    /// Using a type that derives [`serde::Serialize`]:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// #[derive(serde::Serialize)]
    /// struct MyData {
    ///     name: String,
    ///     count: u64,
    /// }
    /// let my_data = MyData { name: "Computers".to_string(), count: 1024 };
    /// let mut req = Request::post("https://example.com");
    /// req.set_body_json(&my_data).unwrap();
    /// assert_eq!(req.get_content_type(), Some(fastly::mime::APPLICATION_JSON));
    /// assert_eq!(&req.into_body_str(), r#"{"name":"Computers","count":1024}"#);
    /// ```
    ///
    /// Using untyped JSON and the [`serde_json::json`] macro:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let my_data = serde_json::json!({
    ///     "name": "Computers",
    ///     "count": 1024,
    /// });
    /// let mut req = Request::post("https://example.com");
    /// req.set_body_json(&my_data).unwrap();
    /// assert_eq!(req.get_content_type(), Some(fastly::mime::APPLICATION_JSON));
    /// assert_eq!(&req.into_body_str(), r#"{"count":1024,"name":"Computers"}"#);
    /// ```
    pub fn set_body_json(&mut self, value: &impl Serialize) -> Result<(), serde_json::Error> {
        self.body = Some(Body::new());
        serde_json::to_writer(self.get_body_mut(), value)?;
        self.set_content_type(mime::APPLICATION_JSON);
        Ok(())
    }

    /// Take the request body and attempt to parse it as a JSON value.
    ///
    /// The return type must implement [`serde::Deserialize`] without any non-static lifetimes. You
    /// can either implement that trait for your own custom type, or use [`serde_json::Value`] to
    /// deserialize untyped JSON values. See [`serde_json`] for details.
    ///
    /// After calling this method, this request will no longer have a body.
    ///
    /// # Errors
    ///
    /// This method returns [`serde_json::Error`] if deserialization fails.
    ///
    /// # Examples
    ///
    /// Using a type that derives [`serde::de::DeserializeOwned`]:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// #[derive(serde::Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     count: u64,
    /// }
    /// let mut req = Request::post("https://example.com")
    ///     .with_body(r#"{"name":"Computers","count":1024}"#);
    /// let my_data = req.take_body_json::<MyData>().unwrap();
    /// assert_eq!(&my_data.name, "Computers");
    /// assert_eq!(my_data.count, 1024);
    /// ```
    ///
    /// Using untyped JSON with [`serde_json::Value`]:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let my_data = serde_json::json!({
    ///     "name": "Computers",
    ///     "count": 1024,
    /// });
    /// let mut req = Request::post("https://example.com")
    ///     .with_body(r#"{"name":"Computers","count":1024}"#);
    /// let my_data = req.take_body_json::<serde_json::Value>().unwrap();
    /// assert_eq!(my_data["name"].as_str(), Some("Computers"));
    /// assert_eq!(my_data["count"].as_u64(), Some(1024));
    /// ```
    pub fn take_body_json<T: DeserializeOwned>(&mut self) -> Result<T, serde_json::Error> {
        if let Some(body) = self.try_take_body() {
            serde_json::from_reader(body)
        } else {
            serde_json::from_reader(std::io::empty())
        }
    }

    /// Builder-style equivalent of [`set_body_form()`][`Self::set_body_form()`].
    pub fn with_body_form(
        mut self,
        value: &impl Serialize,
    ) -> Result<Self, serde_urlencoded::ser::Error> {
        self.set_body_form(value)?;
        Ok(self)
    }

    /// Convert the given value to `application/x-www-form-urlencoded` format and set that data as
    /// the request's body.
    ///
    /// The given value must implement [`serde::Serialize`]; see the trait documentation for
    /// details.
    ///
    #[doc = include_str!("../../docs/snippets/discards-body.md")]
    ///
    /// # Content type
    ///
    /// This method sets the content type to `application/x-www-form-urlencoded`.
    ///
    /// # Errors
    ///
    /// This method returns [`serde_urlencoded::ser::Error`] if serialization fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// #[derive(serde::Serialize)]
    /// struct MyData {
    ///     name: String,
    ///     count: u64,
    /// }
    /// let my_data = MyData { name: "Computers".to_string(), count: 1024 };
    /// let mut req = Request::post("https://example.com");
    /// req.set_body_form(&my_data).unwrap();
    /// assert_eq!(req.get_content_type(), Some(fastly::mime::APPLICATION_WWW_FORM_URLENCODED));
    /// assert_eq!(&req.into_body_str(), "name=Computers&count=1024");
    /// ```
    pub fn set_body_form(
        &mut self,
        value: &impl Serialize,
    ) -> Result<(), serde_urlencoded::ser::Error> {
        self.body = Some(Body::new());
        let s = serde_urlencoded::to_string(value)?;
        self.set_body(s);
        self.set_content_type(mime::APPLICATION_WWW_FORM_URLENCODED);
        Ok(())
    }

    /// Take the request body and attempt to parse it as a `application/x-www-form-urlencoded`
    /// formatted string.
    ///
    #[doc = include_str!("../../docs/snippets/returns-deserializeowned.md")]
    ///
    /// After calling this method, this request will no longer have a body.
    ///
    /// # Errors
    ///
    /// This method returns [`serde_urlencoded::de::Error`] if deserialization fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// #[derive(serde::Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     count: u64,
    /// }
    /// let mut req = Request::post("https://example.com").with_body("name=Computers&count=1024");
    /// let my_data = req.take_body_form::<MyData>().unwrap();
    /// assert_eq!(&my_data.name, "Computers");
    /// assert_eq!(my_data.count, 1024);
    /// ```
    pub fn take_body_form<T: DeserializeOwned>(
        &mut self,
    ) -> Result<T, serde_urlencoded::de::Error> {
        if let Some(body) = self.try_take_body() {
            serde_urlencoded::from_reader(body)
        } else {
            serde_urlencoded::from_reader(std::io::empty())
        }
    }

    /// Get the MIME type described by the request's
    /// [`Content-Type`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Type)
    /// header, or `None` if that header is absent or contains an invalid MIME type.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::post("https://example.com").with_body_text_plain("hello, world!");
    /// assert_eq!(req.get_content_type(), Some(fastly::mime::TEXT_PLAIN_UTF_8));
    /// ```
    pub fn get_content_type(&self) -> Option<Mime> {
        self.get_header_str(http::header::CONTENT_TYPE).map(|v| {
            v.parse()
                .unwrap_or_else(|_| panic!("invalid MIME type in Content-Type header: {}", v))
        })
    }

    /// Builder-style equivalent of [`set_content_type()`][`Self::set_content_type()`].
    pub fn with_content_type(mut self, mime: Mime) -> Self {
        self.set_content_type(mime);
        self
    }

    /// Set the MIME type described by the request's
    /// [`Content-Type`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Type)
    /// header.
    ///
    /// Any existing `Content-Type` header values will be overwritten.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::post("https://example.com").with_body("hello,world!");
    /// req.set_content_type(fastly::mime::TEXT_CSV_UTF_8);
    /// ```
    pub fn set_content_type(&mut self, mime: Mime) {
        self.set_header(http::header::CONTENT_TYPE, mime.as_ref())
    }

    /// Get the value of the request's
    /// [`Content-Length`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Length)
    /// header, if it exists.
    pub fn get_content_length(&self) -> Option<usize> {
        self.get_header(http::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
    }

    /// Returns whether the given header name is present in the request.
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com").with_header("hello", "world!");
    /// assert!(req.contains_header("hello"));
    /// assert!(!req.contains_header("not-present"));
    /// ```
    pub fn contains_header(&self, name: impl ToHeaderName) -> bool {
        self.headers.contains_key(name.into_borrowable().as_ref())
    }

    /// Builder-style equivalent of [`append_header()`][`Self::append_header()`].
    pub fn with_header(mut self, name: impl ToHeaderName, value: impl ToHeaderValue) -> Self {
        self.append_header(name, value);
        self
    }

    /// Builder-style equivalent of [`set_header()`][`Self::set_header()`].
    pub fn with_set_header(mut self, name: impl ToHeaderName, value: impl ToHeaderValue) -> Self {
        self.set_header(name, value);
        self
    }

    /// Get the value of a header as a string, or `None` if the header is not present.
    ///
    /// If there are multiple values for the header, only one is returned, which may be any of the
    /// values. See [`get_header_all_str()`][`Self::get_header_all_str()`] if you need to get all of
    /// the values.
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Panics
    ///
    #[doc = include_str!("../../docs/snippets/panics-reqresp-header-utf8.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com").with_header("hello", "world!");
    /// assert_eq!(req.get_header_str("hello"), Some("world"));
    /// ```
    pub fn get_header_str(&self, name: impl ToHeaderName) -> Option<&str> {
        let name = name.into_borrowable();
        if let Some(hdr) = self.get_header(name.as_ref()) {
            Some(
                hdr.to_str()
                    .unwrap_or_else(|_| panic!("non-UTF-8 HTTP header value for header: {}", name)),
            )
        } else {
            None
        }
    }

    /// Get the value of a header as a string, including invalid characters, or `None` if the header
    /// is not present.
    ///
    #[doc = include_str!("../../docs/snippets/utf8-replacement.md")]
    ///
    /// If there are multiple values for the header, only one is returned, which may be any of the
    /// values. See [`get_header_all_str_lossy()`][`Self::get_header_all_str_lossy()`] if you need
    /// to get all of the values.
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// # use http::header::HeaderValue;
    /// # use std::borrow::Cow;
    /// let header_value = HeaderValue::from_bytes(b"\xF0\x90\x80 world!").unwrap();
    /// let req = Request::get("https://example.com").with_header("hello", header_value);
    /// assert_eq!(req.get_header_str_lossy("hello"), Some(Cow::from("� world")));
    /// ```
    pub fn get_header_str_lossy(&self, name: impl ToHeaderName) -> Option<Cow<'_, str>> {
        self.get_header(name)
            .map(|hdr| String::from_utf8_lossy(hdr.as_bytes()))
    }

    /// Get the value of a header, or `None` if the header is not present.
    ///
    /// If there are multiple values for the header, only one is returned, which may be any of the
    /// values. See [`get_header_all()`][`Self::get_header_all()`] if you need to get all of the
    /// values.
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Examples
    ///
    /// Handling UTF-8 values explicitly:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// # use fastly::http::HeaderValue;
    /// let req = Request::get("https://example.com").with_header("hello", "world!");
    /// assert_eq!(req.get_header("hello"), Some(&HeaderValue::from_static("world")));
    /// ```
    ///
    /// Safely handling invalid UTF-8 values:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let invalid_utf8 = &"🐈".as_bytes()[0..3];
    /// let req = Request::get("https://example.com").with_header("hello", invalid_utf8);
    /// assert_eq!(req.get_header("hello").unwrap().as_bytes(), invalid_utf8);
    /// ```
    pub fn get_header(&self, name: impl ToHeaderName) -> Option<&HeaderValue> {
        self.headers.get(name.into_borrowable().as_ref())
    }

    /// Get all values of a header as strings, or an empty vector if the header is not present.
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Panics
    ///
    #[doc = include_str!("../../docs/snippets/panics-reqresp-headers-utf8.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com")
    ///     .with_header("hello", "world!")
    ///     .with_header("hello", "universe!");
    /// let values = req.get_header_all_str("hello");
    /// assert_eq!(values.len(), 2);
    /// assert!(values.contains(&"world!"));
    /// assert!(values.contains(&"universe!"));
    /// ```
    pub fn get_header_all_str(&self, name: impl ToHeaderName) -> Vec<&str> {
        let name = name.into_borrowable();
        self.get_header_all(name.as_ref())
            .map(|v| {
                v.to_str()
                    .unwrap_or_else(|_| panic!("non-UTF-8 HTTP header value for header: {}", name))
            })
            .collect()
    }

    /// Get all values of a header as strings, including invalid characters, or an empty vector if the header is not present.
    ///
    #[doc = include_str!("../../docs/snippets/utf8-replacement.md")]
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::borrow::Cow;
    /// # use http::header::HeaderValue;
    /// # use fastly::Request;
    /// let world_value = HeaderValue::from_bytes(b"\xF0\x90\x80 world!").unwrap();
    /// let universe_value = HeaderValue::from_bytes(b"\xF0\x90\x80 universe!").unwrap();
    /// let req = Request::get("https://example.com")
    ///     .with_header("hello", world_value)
    ///     .with_header("hello", universe_value);
    /// let values = req.get_header_all_str_lossy("hello");
    /// assert_eq!(values.len(), 2);
    /// assert!(values.contains(&Cow::from("� world!")));
    /// assert!(values.contains(&Cow::from("� universe!")));
    /// ```
    pub fn get_header_all_str_lossy(&self, name: impl ToHeaderName) -> Vec<Cow<'_, str>> {
        self.get_header_all(name)
            .map(|hdr| String::from_utf8_lossy(hdr.as_bytes()))
            .collect()
    }

    /// Get an iterator of all the values of a header.
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Examples
    ///
    /// You can turn the iterator into collection, like [`Vec`]:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// # use fastly::http::HeaderValue;
    /// let invalid_utf8 = &"🐈".as_bytes()[0..3];
    /// let req = Request::get("https://example.com")
    ///     .with_header("hello", "world!")
    ///     .with_header("hello", invalid_utf8);
    ///
    /// let values: Vec<&HeaderValue> = req.get_header_all("hello").collect();
    /// assert_eq!(values.len(), 2);
    /// assert!(values.contains(&&HeaderValue::from_static("world!")));
    /// assert!(values.contains(&&HeaderValue::from_bytes(invalid_utf8).unwrap()));
    /// ```
    ///
    /// You can use the iterator in a loop:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let invalid_utf8 = &"🐈".as_bytes()[0..3];
    /// let req = Request::get("https://example.com")
    ///     .with_header("hello", "world!")
    ///     .with_header("hello", invalid_utf8);
    ///
    /// for value in req.get_header_all("hello") {
    ///     if let Ok(s) = value.to_str() {
    ///         println!("hello, {}", s);
    ///     } else {
    ///         println!("hello, invalid UTF-8!");
    ///     }
    /// }
    /// ```
    pub fn get_header_all(&self, name: impl ToHeaderName) -> impl Iterator<Item = &HeaderValue> {
        self.headers.get_all(name.into_borrowable().as_ref()).iter()
    }

    /// Get an iterator of all the request's header names and values.
    ///
    /// # Examples
    ///
    /// You can turn the iterator into a collection, like [`Vec`]:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// # use fastly::http::header::{HeaderName, HeaderValue};
    /// let req = Request::get("https://example.com")
    ///     .with_header("hello", "world!")
    ///     .with_header("hello", "universe!");
    ///
    /// let headers: Vec<(&HeaderName, &HeaderValue)> = req.get_headers().collect();
    /// assert_eq!(headers.len(), 2);
    /// assert!(headers.contains(&(&HeaderName::from_static("hello"), &HeaderValue::from_static("world!"))));
    /// assert!(headers.contains(&(&HeaderName::from_static("hello"), &HeaderValue::from_static("universe!"))));
    /// ```
    ///
    /// You can use the iterator in a loop:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com")
    ///     .with_header("hello", "world!")
    ///     .with_header("hello", "universe!");
    ///
    /// for (n, v) in req.get_headers() {
    ///     println!("Header -  {}: {:?}", n, v);
    /// }
    /// ```
    pub fn get_headers(&self) -> impl Iterator<Item = (&HeaderName, &HeaderValue)> {
        self.headers.iter()
    }

    /// Get all of the request's header names as strings, or an empty vector if no headers are
    /// present.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com")
    ///     .with_header("hello", "world!")
    ///     .with_header("goodbye", "latency!");
    /// let names = req.get_header_names_str();
    /// assert_eq!(names.len(), 2);
    /// assert!(names.contains(&"hello"));
    /// assert!(names.contains(&"goodbye"));
    /// ```
    pub fn get_header_names_str(&self) -> Vec<&str> {
        self.get_header_names().map(|n| n.as_str()).collect()
    }

    /// Get an iterator of all the request's header names.
    ///
    /// # Examples
    ///
    /// You can turn the iterator into collection, like [`Vec`]:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// # use fastly::http::header::HeaderName;
    /// let req = Request::get("https://example.com")
    ///     .with_header("hello", "world!")
    ///     .with_header("goodbye", "latency!");
    ///
    /// let values: Vec<&HeaderName> = req.get_header_names().collect();
    /// assert_eq!(values.len(), 2);
    /// assert!(values.contains(&&HeaderName::from_static("hello")));
    /// assert!(values.contains(&&HeaderName::from_static("goodbye")));
    /// ```
    ///
    /// You can use the iterator in a loop:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com")
    ///     .with_header("hello", "world!")
    ///     .with_header("goodbye", "latency!");
    ///
    /// for name in req.get_header_names() {
    ///     println!("saw header: {:?}", name);
    /// }
    /// ```
    pub fn get_header_names(&self) -> impl Iterator<Item = &HeaderName> {
        self.headers.keys()
    }

    /// Set a request header to the given value, discarding any previous values for the given
    /// header name.
    ///
    #[doc = include_str!("../../docs/snippets/header-name-value-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::get("https://example.com");
    ///
    /// req.set_header("hello", "world!");
    /// assert_eq!(req.get_header_str("hello"), Some("world!"));
    ///
    /// req.set_header("hello", "universe!");
    ///
    /// let values = req.get_header_all_str("hello");
    /// assert_eq!(values.len(), 1);
    /// assert!(!values.contains(&"world!"));
    /// assert!(values.contains(&"universe!"));
    /// ```
    pub fn set_header(&mut self, name: impl ToHeaderName, value: impl ToHeaderValue) {
        self.headers.insert(name.into_owned(), value.into_owned());
    }

    /// Add a request header with given value.
    ///
    /// Unlike [`set_header()`][`Self::set_header()`], this does not discard existing values for the
    /// same header name.
    ///
    #[doc = include_str!("../../docs/snippets/header-name-value-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::get("https://example.com");
    ///
    /// req.set_header("hello", "world!");
    /// assert_eq!(req.get_header_str("hello"), Some("world!"));
    ///
    /// req.append_header("hello", "universe!");
    ///
    /// let values = req.get_header_all_str("hello");
    /// assert_eq!(values.len(), 2);
    /// assert!(values.contains(&"world!"));
    /// assert!(values.contains(&"universe!"));
    /// ```
    pub fn append_header(&mut self, name: impl ToHeaderName, value: impl ToHeaderValue) {
        self.headers.append(name.into_owned(), value.into_owned());
    }

    /// Remove all request headers of the given name, and return one of the removed header values
    /// if any were present.
    ///
    #[doc = include_str!("../../docs/snippets/removes-one-header.md")]
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// # use fastly::http::HeaderValue;
    /// let mut req = Request::get("https://example.com").with_header("hello", "world!");
    /// assert_eq!(req.get_header_str("hello"), Some("world!"));
    /// assert_eq!(req.remove_header("hello"), Some(HeaderValue::from_static("world!")));
    /// assert!(req.remove_header("not-present").is_none());
    /// ```
    pub fn remove_header(&mut self, name: impl ToHeaderName) -> Option<HeaderValue> {
        self.headers.remove(name.into_borrowable().as_ref())
    }

    /// Remove all request headers of the given name, and return one of the removed header values as
    /// a string if any were present.
    ///
    #[doc = include_str!("../../docs/snippets/removes-one-header.md")]
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Panics
    ///
    #[doc = include_str!("../../docs/snippets/panics-reqresp-remove-header-utf8.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::get("https://example.com").with_header("hello", "world!");
    /// assert_eq!(req.get_header_str("hello"), Some("world!"));
    /// assert_eq!(req.remove_header_str("hello"), Some("world!".to_string()));
    /// assert!(req.remove_header_str("not-present").is_none());
    /// ```
    pub fn remove_header_str(&mut self, name: impl ToHeaderName) -> Option<String> {
        let name = name.into_borrowable();
        if let Some(hdr) = self.remove_header(name.as_ref()) {
            Some(
                hdr.to_str()
                    .map(|s| s.to_owned())
                    .unwrap_or_else(|_| panic!("non-UTF-8 HTTP header value for header: {}", name)),
            )
        } else {
            None
        }
    }

    /// Remove all request headers of the given name, and return one of the removed header values
    /// as a string, including invalid characters, if any were present.
    ///
    #[doc = include_str!("../../docs/snippets/utf8-replacement.md")]
    ///
    #[doc = include_str!("../../docs/snippets/removes-one-header.md")]
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// # use http::header::HeaderValue;
    /// # use std::borrow::Cow;
    /// let header_value = HeaderValue::from_bytes(b"\xF0\x90\x80 world!").unwrap();
    /// let mut req = Request::get("https://example.com")
    ///     .with_header("hello", header_value);
    /// assert_eq!(req.get_header_str_lossy("hello"), Some(Cow::from("� world")));
    /// assert_eq!(req.remove_header_str_lossy("hello"), Some(String::from("� world")));
    /// assert!(req.remove_header_str_lossy("not-present").is_none());
    /// ```
    pub fn remove_header_str_lossy(&mut self, name: impl ToHeaderName) -> Option<String> {
        self.remove_header(name)
            .map(|hdr| String::from_utf8_lossy(hdr.as_bytes()).into_owned())
    }

    /// Builder-style equivalent of [`set_method()`][`Self::set_method()`].
    pub fn with_method(mut self, method: impl ToMethod) -> Self {
        self.set_method(method);
        self
    }

    /// Get the request method as a string.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com");
    /// assert_eq!(req.get_method_str(), "GET");
    pub fn get_method_str(&self) -> &str {
        self.get_method().as_str()
    }

    /// Get the request method.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// use fastly::http::Method;
    /// fn log_method(req: &Request) {
    ///     match req.get_method() {
    ///         &Method::GET | &Method::HEAD => println!("method was a GET or HEAD"),
    ///         &Method::POST => println!("method was a POST"),
    ///         _ => println!("method was something else"),
    ///     }
    /// }
    pub fn get_method(&self) -> &Method {
        &self.method
    }

    /// Set the request method.
    ///
    #[doc = include_str!("../../docs/snippets/method-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// use fastly::http::Method;
    ///
    /// let mut req = Request::get("https://example.com");
    /// req.set_method(Method::POST);
    /// assert_eq!(req.get_method(), &Method::POST);
    /// ```
    pub fn set_method<'a>(&mut self, method: impl ToMethod) {
        self.method = method.into_owned();
    }

    /// Builder-style equivalent of [`set_url()`][`Self::set_url()`].
    pub fn with_url(mut self, url: impl ToUrl) -> Self {
        self.set_url(url);
        self
    }

    /// Get the request URL as a string.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com");
    /// assert_eq!(req.get_url_str(), "https://example.com");
    /// ```
    pub fn get_url_str(&self) -> &str {
        self.get_url().as_str()
    }

    /// Get a shared reference to the request URL.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com/hello#world");
    /// let url = req.get_url();
    /// assert_eq!(url.host_str(), Some("example.com"));
    /// assert_eq!(url.path(), "/hello");
    /// assert_eq!(url.fragment(), Some("world"));
    /// ```
    pub fn get_url(&self) -> &Url {
        &self.url
    }

    /// Get a mutable reference to the request URL.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::get("https://example.com/");
    ///
    /// let url = req.get_url_mut();
    /// url.set_path("/hello");
    /// url.set_fragment(Some("world"));
    ///
    /// assert_eq!(req.get_url_str(), "https://example.com/hello#world");
    /// ```
    pub fn get_url_mut(&mut self) -> &mut Url {
        &mut self.url
    }

    /// Set the request URL.
    ///
    #[doc = include_str!("../../docs/snippets/url-argument.md")]
    pub fn set_url(&mut self, url: impl ToUrl) {
        self.url = url.into_owned();
    }

    /// Get the path component of the request URL.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com/hello#world");
    /// assert_eq!(req.get_path(), "/hello");
    /// ```
    pub fn get_path(&self) -> &str {
        self.get_url().path()
    }

    /// Builder-style equivalent of [`set_path()`][`Self::set_path()`].
    pub fn with_path(mut self, path: &str) -> Self {
        self.set_path(path);
        self
    }

    /// Set the path component of the request URL.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::get("https://example.com/");
    /// req.set_path("/hello");
    /// assert_eq!(req.get_url_str(), "https://example.com/hello");
    /// ```
    pub fn set_path(&mut self, path: &str) {
        self.get_url_mut().set_path(path);
    }

    /// Get the query component of the request URL, if it exists, as a percent-encoded ASCII string.
    ///
    /// This is a shorthand for `self.get_url().query()`; see [`Url::query()`] for details and other
    /// query manipulation functions.
    pub fn get_query_str(&self) -> Option<&str> {
        self.get_url().query()
    }

    /// Get the value of a query parameter in the request's URL.
    ///
    /// This assumes that the query string is a `&` separated list of `parameter=value` pairs.  The
    /// value of the first occurrence of `parameter` is returned. No URL decoding is performed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com/page?foo=bar&baz=qux");
    /// assert_eq!(req.get_query_parameter("foo"), Some("bar"));
    /// assert_eq!(req.get_query_parameter("baz"), Some("qux"));
    /// assert_eq!(req.get_query_parameter("other"), None);
    /// ```
    pub fn get_query_parameter(&self, parameter: &str) -> Option<&str> {
        self.get_url().query().and_then(|qs| {
            qs.split('&').find_map(|part| {
                part.strip_prefix(parameter)
                    .and_then(|maybe| maybe.strip_prefix('='))
            })
        })
    }

    /// Attempt to parse the query component of the request URL into the specified datatype.
    ///
    #[doc = include_str!("../../docs/snippets/returns-deserializeowned.md")]
    ///
    /// # Errors
    ///
    /// This method returns [`serde_urlencoded::de::Error`] if deserialization fails.
    ///
    /// # Examples
    ///
    /// Parsing into a vector of string pairs:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let req = Request::get("https://example.com/foo?hello=%F0%9F%8C%90!&bar=baz");
    /// let pairs: Vec<(String, String)> = req.get_query().unwrap();
    /// assert_eq!((pairs[0].0.as_str(), pairs[0].1.as_str()), ("hello", "🌐!"));
    /// ```
    ///
    /// Parsing into a mapping between strings (note that duplicates are removed since
    /// [`HashMap`][`std::collections::HashMap`] is not a multimap):
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// use std::collections::HashMap;
    /// let req = Request::get("https://example.com/foo?hello=%F0%9F%8C%90!&bar=baz&bar=quux");
    /// let map: HashMap<String, String> = req.get_query().unwrap();
    /// assert_eq!(map.len(), 2);
    /// assert_eq!(map["hello"].as_str(), "🌐!");
    /// ```
    ///
    /// Parsing into a custom type that derives [`serde::de::Deserialize`]:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// #[derive(serde::Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     count: u64,
    /// }
    /// let mut req = Request::get("https://example.com/?name=Computers&count=1024");
    /// let my_data = req.take_body_form::<MyData>().unwrap();
    /// assert_eq!(&my_data.name, "Computers");
    /// assert_eq!(my_data.count, 1024);
    /// ```
    pub fn get_query<T: DeserializeOwned>(&self) -> Result<T, serde_urlencoded::de::Error> {
        serde_urlencoded::from_str(self.url.query().unwrap_or(""))
    }

    /// Builder-style equivalent of [`set_query_str()`][`Self::set_query_str()`].
    pub fn with_query_str(mut self, query: impl AsRef<str>) -> Self {
        self.set_query_str(query);
        self
    }

    /// Set the query string of the request URL query component to the given string, performing
    /// percent-encoding if necessary.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::get("https://example.com/foo");
    /// req.set_query_str("hello=🌐!&bar=baz");
    /// assert_eq!(req.get_url_str(), "https://example.com/foo?hello=%F0%9F%8C%90!&bar=baz");
    /// ```
    pub fn set_query_str(&mut self, query: impl AsRef<str>) {
        self.get_url_mut().set_query(Some(query.as_ref()))
    }

    /// Builder-style equivalent of [`set_query()`][`Self::set_query()`].
    pub fn with_query(
        mut self,
        query: &impl Serialize,
    ) -> Result<Self, serde_urlencoded::ser::Error> {
        self.set_query(query)?;
        Ok(self)
    }

    /// Convert the given value to `application/x-www-form-urlencoded` format and set that data as
    /// the request URL query component.
    ///
    /// The given value must implement [`serde::Serialize`]; see the trait documentation for
    /// details.
    ///
    /// # Errors
    ///
    /// This method returns [`serde_urlencoded::ser::Error`] if serialization fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// #[derive(serde::Serialize)]
    /// struct MyData {
    ///     name: String,
    ///     count: u64,
    /// }
    /// let my_data = MyData { name: "Computers".to_string(), count: 1024 };
    /// let mut req = Request::get("https://example.com/foo");
    /// req.set_query(&my_data).unwrap();
    /// assert_eq!(req.get_url_str(), "https://example.com/foo?name=Computers&count=1024");
    /// ```
    pub fn set_query(
        &mut self,
        query: &impl Serialize,
    ) -> Result<(), serde_urlencoded::ser::Error> {
        let s = serde_urlencoded::to_string(query)?;
        self.get_url_mut().set_query(Some(&s));
        Ok(())
    }

    /// Remove the query component from the request URL, if one exists.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut req = Request::get("https://example.com/foo?hello=%F0%9F%8C%90!&bar=baz");
    /// req.remove_query();
    /// assert_eq!(req.get_url_str(), "https://example.com/foo");
    /// ```
    pub fn remove_query(&mut self) {
        self.get_url_mut().set_query(None);
    }

    /// Builder-style equivalent of [`set_version()`][`Self::set_version()`].
    pub fn with_version(mut self, version: Version) -> Self {
        self.set_version(version);
        self
    }

    /// Get the HTTP version of this request.
    pub fn get_version(&self) -> Version {
        self.version
    }

    /// Set the HTTP version of this request.
    pub fn set_version(&mut self, version: Version) {
        self.version = version;
    }

    /// Builder-style equivalent of [`set_pass()`][`Self::set_pass()`].
    pub fn with_pass(mut self, pass: bool) -> Self {
        self.set_pass(pass);
        self
    }

    /// Set whether this request should be cached if sent to a backend.
    ///
    /// By default this is `false`, which means the backend will only be reached if a cached
    /// response is not available. Set this to `true` to send the request directly to the backend
    /// without caching.
    ///
    /// # Overrides
    ///
    /// Setting this to `true` overrides any other custom caching behaviors for this request, such
    /// as [`Request::set_ttl()`] or [`Request::set_surrogate_key()`].
    pub fn set_pass(&mut self, pass: bool) {
        self.cache_override.set_pass(pass);
    }

    /// Builder-style equivalent of [`set_ttl()`][`Self::set_ttl()`].
    pub fn with_ttl(mut self, ttl: u32) -> Self {
        self.set_ttl(ttl);
        self
    }

    /// Override the caching behavior of this request to use the given Time to Live (TTL), in seconds.
    ///
    /// # Overrides
    ///
    /// This overrides the behavior specified in the response headers, and sets the
    /// [`pass`][`Self::set_pass()`] behavior to `false`.
    pub fn set_ttl(&mut self, ttl: u32) {
        self.cache_override.set_ttl(ttl);
    }

    /// Builder-style equivalent of [`set_stale_while_revalidate()`][`Self::set_stale_while_revalidate()`].
    pub fn with_stale_while_revalidate(mut self, swr: u32) -> Self {
        self.set_stale_while_revalidate(swr);
        self
    }

    /// Override the caching behavior of this request to use the given `stale-while-revalidate`
    /// time, in seconds.
    ///
    /// # Overrides
    ///
    /// This overrides the behavior specified in the response headers, and sets the
    /// [`pass`][`Self::set_pass()`] behavior to `false`.
    pub fn set_stale_while_revalidate(&mut self, swr: u32) {
        self.cache_override.set_stale_while_revalidate(swr);
    }

    /// Builder-style equivalent of [`set_pci()`][`Self::set_pci()`].
    pub fn with_pci(mut self, pci: bool) -> Self {
        self.set_pci(pci);
        self
    }

    /// Override the caching behavior of this request to enable or disable PCI/HIPAA-compliant
    /// non-volatile caching.
    ///
    /// By default, this is `false`, which means the request may not be PCI/HIPAA-compliant. Set it
    /// to `true` to enable compliant caching.
    ///
    /// See the [Fastly PCI-Compliant Caching and Delivery
    /// documentation](https://docs.fastly.com/products/pci-compliant-caching-and-delivery) for
    /// details.
    ///
    /// # Overrides
    ///
    /// This sets the [`pass`][`Self::set_pass()`] behavior to `false`.
    pub fn set_pci(&mut self, pci: bool) {
        self.cache_override.set_pci(pci);
    }

    /// Builder-style equivalent of [`set_surrogate_key()`][`Self::set_surrogate_key()`].
    pub fn with_surrogate_key(mut self, sk: HeaderValue) -> Self {
        self.set_surrogate_key(sk);
        self
    }

    /// Override the caching behavior of this request to include the given surrogate key(s),
    /// provided as a header value.
    ///
    /// The header value can contain more than one surrogate key, separated by spaces.
    ///
    /// Surrogate keys must contain only printable ASCII characters (those between `0x21` and
    /// `0x7E`, inclusive). Any invalid keys will be ignored.
    ///
    /// See the [Fastly surrogate keys
    /// guide](https://docs.fastly.com/en/guides/purging-api-cache-with-surrogate-keys) for details.
    ///
    /// # Overrides
    ///
    /// This sets the [`pass`][`Self::set_pass()`] behavior to `false`, and extends (but does not
    /// replace) any `Surrogate-Key` response headers from the backend.
    pub fn set_surrogate_key(&mut self, sk: HeaderValue) {
        self.cache_override.set_surrogate_key(sk);
    }

    /// Returns the IP address of the client making the HTTP request.
    ///
    /// Returns `None` if this is not the client request.
    pub fn get_client_ip_addr(&self) -> Option<IpAddr> {
        if !self.is_from_client() {
            return None;
        }
        self::handle::client_ip_addr()
    }

    /// Returns the client request's header names exactly as they were originally received.
    ///
    /// This includes both the original character cases, as well as the original order of the
    /// received headers.
    ///
    /// Returns `None` if this is not the client request.
    pub fn get_original_header_names(&self) -> Option<impl Iterator<Item = String>> {
        if !self.is_from_client() {
            return None;
        } else {
            Some(
                self::handle::client_original_header_names_impl(
                    limits::INITIAL_HEADER_NAME_BUF_SIZE,
                    RequestLimits::get_max_header_name_bytes(),
                )
                .map(|res| res.expect("original request header name too large")),
            )
        }
    }

    /// Returns the number of headers in the client request as originally received.
    ///
    /// Returns `None` if this is not the client request.
    pub fn get_original_header_count(&self) -> Option<u32> {
        if !self.is_from_client() {
            return None;
        }
        Some(self::handle::client_original_header_count())
    }

    /// Get the HTTP/2 fingerprint of client request if available
    ///
    /// Returns `None` if this is not the client request.
    pub fn get_client_h2_fingerprint(&self) -> Option<&str> {
        if !self.is_from_client() {
            return None;
        }

        self::handle::client_h2_fingerprint()
    }

    /// Get the request id of the current request if available
    ///
    /// Returns `None` if this is not the client request.
    pub fn get_client_request_id(&self) -> Option<&str> {
        if !self.is_from_client() {
            return None;
        }

        self::handle::client_request_id()
    }

    /// Get the raw bytes sent by the client in the TLS ClientHello message.
    ///
    /// See [RFC 5246](https://tools.ietf.org/html/rfc5246#section-7.4.1.2) for details.
    ///
    /// Returns `None` if this is not the client request.
    pub fn get_tls_client_hello(&self) -> Option<&[u8]> {
        if !self.is_from_client() {
            return None;
        }

        self::handle::client_tls_client_hello()
    }

    /// Get the JA3 hash of the TLS ClientHello message.
    ///
    /// Returns `None` if this is not available.
    pub fn get_tls_ja3_md5(&self) -> Option<[u8; 16]> {
        if !self.is_from_client() {
            return None;
        }

        self::handle::client_tls_ja3_md5()
    }

    /// Get the raw client certificate in the mutual TLS handshake message.
    /// It is in PEM format.
    /// Returns `None` if this is not mTLS or available.
    pub fn get_tls_raw_client_certificate(&self) -> Option<&'static str> {
        if !self.is_from_client() {
            return None;
        }

        self::handle::client_tls_client_raw_certificate()
    }

    /// Returns the error code defined in ClientCertVerifyResult.
    ///
    /// Returns `None` if this is not available.
    pub fn get_tls_client_cert_verify_result(&self) -> Option<ClientCertVerifyResult> {
        if !self.is_from_client() {
            return None;
        }
        self::handle::client_tls_client_cert_verify_result()
    }

    /// Get the cipher suite used to secure the client TLS connection.
    ///
    /// The value returned will be consistent with the [OpenSSL
    /// name](https://testssl.sh/openssl-iana.mapping.html) for the cipher suite.
    ///
    /// Returns `None` if this is not the client request.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// assert_eq!(Request::from_client().get_tls_cipher_openssl_name().unwrap(), "ECDHE-RSA-AES128-GCM-SHA256");
    /// ```
    pub fn get_tls_cipher_openssl_name(&self) -> Option<&'static str> {
        if !self.is_from_client() {
            return None;
        }

        self::handle::client_tls_cipher_openssl_name()
    }

    /// Get the TLS protocol version used to secure the client TLS connection.
    ///
    /// Returns `None` if this is not the client request.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// assert_eq!(Request::from_client().get_tls_protocol().unwrap(), "TLSv1.2");
    /// ```
    pub fn get_tls_protocol(&self) -> Option<&'static str> {
        if !self.is_from_client() {
            return None;
        }
        self::handle::client_tls_protocol()
    }

    /// Set whether a `gzip`-encoded response to this request will be automatically decompressed.
    ///
    /// If the response to this request is `gzip`-encoded, it will be presented in decompressed
    /// form, and the `Content-Encoding` and `Content-Length` headers will be removed.
    pub fn set_auto_decompress_gzip(&mut self, gzip: bool) {
        self.auto_decompress_response
            .set(ContentEncodings::GZIP, gzip);
    }

    /// Builder-style equivalent of
    /// [`set_auto_decompress_gzip()`][`Self::set_auto_decompress_gzip()`].
    pub fn with_auto_decompress_gzip(mut self, gzip: bool) -> Self {
        self.set_auto_decompress_gzip(gzip);
        self
    }

    /// Sets how `Content-Length` and `Transfer-Encoding` will be determined when sending this
    /// request.
    ///
    /// See [`FramingHeadersMode`] for details on the options.
    pub fn set_framing_headers_mode(&mut self, mode: FramingHeadersMode) {
        self.framing_headers_mode = mode;
    }

    /// Builder-style equivalent of
    /// [`set_framing_headers_mode()`][`Self::set_framing_headers_mode()`].
    pub fn with_framing_headers_mode(mut self, mode: FramingHeadersMode) -> Self {
        self.set_framing_headers_mode(mode);
        self
    }

    /// Create a [`Request`] from the low-level [`handle` API][`crate::handle`].
    ///
    /// # Errors
    ///
    /// This conversion can fail if the request exceeds the limits specified by [`RequestLimits`].
    pub fn from_handles(
        req_handle: RequestHandle,
        body_handle: Option<BodyHandle>,
    ) -> Result<Self, BufferSizeError> {
        let req_limits = limits::REQUEST_LIMITS.read().unwrap();
        let method = req_handle
            .get_method_impl(limits::INITIAL_METHOD_BUF_SIZE, req_limits.max_method_bytes)?;
        let url =
            req_handle.get_url_impl(limits::INITIAL_URL_BUF_SIZE, req_limits.max_url_bytes)?;

        let mut req = Request::new(method, url).with_version(req_handle.get_version());
        req.is_from_client = true;

        for name in req_handle.get_header_names_impl(
            limits::INITIAL_HEADER_NAME_BUF_SIZE,
            req_limits.max_header_name_bytes,
        ) {
            let name = name?;
            for value in req_handle.get_header_values_impl(
                &name,
                limits::INITIAL_HEADER_VALUE_BUF_SIZE,
                req_limits.max_header_value_bytes,
            ) {
                let value = value?;
                req.append_header(&name, value);
            }
        }

        if let Some(body) = body_handle {
            req.set_body(body);
        }
        Ok(req)
    }

    /// Convert a [`Request`] into the low-level [`handle` API][`crate::handle`].
    pub fn into_handles(mut self) -> (RequestHandle, Option<BodyHandle>) {
        self.to_handles()
    }

    /// Make handles from a `Request`.
    ///
    /// Note that this is private in order to maintain the right ownership model in the public API.
    fn to_handles(&mut self) -> (RequestHandle, Option<BodyHandle>) {
        let req_handle = {
            let mut req_handle = RequestHandle::new();
            // Set the handle's version, method, URI, cache override, and auto decompression
            // settings using the request.
            req_handle.set_version(self.version);
            req_handle.set_method(&self.method);
            req_handle.set_url(&self.url);
            req_handle.set_cache_override(&self.cache_override);
            req_handle.set_auto_decompress_response(self.auto_decompress_response);
            req_handle.set_framing_headers_mode(self.framing_headers_mode);
            for name in self.headers.keys() {
                // Copy the request's header values to the handle.
                req_handle.set_header_values(name, self.headers.get_all(name));
            }

            // For now, we'll only compute cache keys for services opting into the experiment.
            if let Some(exp_key_override) = self.cache_key.as_ref().map(|gen| match gen {
                CacheKeyGen::Lazy(f) => f(self),
                CacheKeyGen::Set(k) => *k,
            }) {
                use crate::experimental::RequestHandleCacheKey;
                req_handle.set_cache_key(&exp_key_override);
            }

            req_handle
        };
        let body_handle = if let Some(body) = self.try_take_body() {
            Some(body.into_handle())
        } else {
            None
        };
        (req_handle, body_handle)
    }

    /// Returns whether or not the client request had a `Fastly-Key` header which is valid for
    /// purging content for the service.
    ///
    /// This function ignores the current value of any `Fastly-Key` header for this request.
    pub fn fastly_key_is_valid(&self) -> bool {
        if !self.is_from_client() {
            return false;
        }
        self::handle::fastly_key_is_valid()
    }
}

/// Anything that we need to make a full roundtrip through the `http` types that doesn't have a more
/// concrete corresponding type.
#[derive(Debug, Default)]
struct FastlyExts {
    cache_override: CacheOverride,
    is_from_client: bool,
    auto_decompress_response: ContentEncodings,
    framing_headers_mode: FramingHeadersMode,
    cache_key: Option<CacheKeyGen>,
}

impl Into<http::Request<Body>> for Request {
    fn into(self) -> http::Request<Body> {
        let mut req = http::Request::new(self.body.unwrap_or_else(|| Body::new()));
        req.extensions_mut().insert(FastlyExts {
            cache_override: self.cache_override,
            is_from_client: self.is_from_client,
            auto_decompress_response: self.auto_decompress_response,
            framing_headers_mode: self.framing_headers_mode,
            cache_key: self.cache_key,
        });
        *req.headers_mut() = self.headers;
        *req.method_mut() = self.method;
        *req.uri_mut() = String::from(self.url)
            .parse()
            .expect("Url to Uri conversion shouldn't fail, but did");
        *req.version_mut() = self.version;
        req
    }
}

impl From<http::Request<Body>> for Request {
    fn from(from: http::Request<Body>) -> Self {
        let (mut parts, body) = from.into_parts();
        let FastlyExts {
            cache_override,
            is_from_client,
            auto_decompress_response,
            framing_headers_mode,
            cache_key,
        } = parts.extensions.remove().unwrap_or_default();
        Request {
            version: parts.version,
            method: parts.method,
            url: Url::parse(&parts.uri.to_string())
                .expect("Uri to Url conversion shouldn't fail, but did"),
            headers: parts.headers,
            body: Some(body),
            cache_override,
            is_from_client,
            auto_decompress_response,
            framing_headers_mode,
            cache_key,
        }
    }
}

/// The reason that a request sent to a backend failed.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum SendErrorCause {
    /// The backend responded with something that was not valid HTTP.
    Invalid,
    /// The backend connection closed before a complete response could be read.
    Incomplete,
    /// The backend responded with an invalid HTTP code.
    InvalidStatus,
    /// The backend responded with a HTTP message head that was too large.
    HeadTooLarge,
    /// Ran out of buffer space for part of the response.
    ///
    /// See the [`limits`][crate::limits] module to adjust the maximum buffer sizes.
    BufferSize(BufferSizeError),
    /// All other errors.
    Generic(Error),
}

impl fmt::Display for SendErrorCause {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SendErrorCause::Invalid => {
                write!(f, "response was invalid HTTP")
            }
            SendErrorCause::Incomplete => {
                write!(f, "response was not a complete HTTP message")
            }
            SendErrorCause::HeadTooLarge => {
                write!(f, "response message head was too large")
            }
            SendErrorCause::InvalidStatus => {
                write!(f, "response status line was invalid")
            }
            SendErrorCause::BufferSize(buffer_size_error) => {
                write!(f, "response included a {} that exceeded a provided buffer's capacity (needed {} bytes)", buffer_size_error.buffer_kind, buffer_size_error.needed_buf_size)
            }
            SendErrorCause::Generic(e) => {
                write!(f, "generic send error: {}", e)
            }
        }
    }
}

impl SendErrorCause {
    pub(crate) fn status(cause: fastly_shared::FastlyStatus) -> Self {
        match cause {
            fastly_shared::FastlyStatus::HTTPINVALID => SendErrorCause::Invalid,
            fastly_shared::FastlyStatus::HTTPINCOMPLETE => SendErrorCause::Incomplete,
            fastly_shared::FastlyStatus::HTTPHEADTOOLARGE => SendErrorCause::HeadTooLarge,
            fastly_shared::FastlyStatus::HTTPINVALIDSTATUS => SendErrorCause::InvalidStatus,
            fastly_shared::FastlyStatus::ERROR => {
                SendErrorCause::Generic(Error::msg(format!("Error occurred processing send")))
            }
            other => SendErrorCause::Generic(Error::msg(format!(
                "SendError with unknown FastlyStatus code: {}",
                other.code
            ))),
        }
    }
}

/// An error that occurred while sending a request.
///
/// While the body of a request is always consumed when sent, you can recover the headers and other
/// request metadata of the request that failed using `SendError::into_sent_req()`.
///
/// use [`SendError::root_cause()`] to inspect details about what caused the error.
#[derive(Debug, Error)]
#[error("error sending request: {error} to backend {backend}")]
pub struct SendError {
    backend: String,
    sent_req: Request,
    #[source]
    error: SendErrorCause,
}

impl SendError {
    pub(crate) fn new(
        backend: impl Into<String>,
        sent_req: Request,
        error: SendErrorCause,
    ) -> Self {
        SendError {
            backend: backend.into(),
            sent_req,
            error: error.into(),
        }
    }

    /// Create a `SendError` from a `FastlyResponseMetadata` and an underlying error.
    ///
    /// Panics if the metadata does not contain a backend and a sent request. This should only be
    /// called in contexts where those are guaranteed to be present, like the metadata from a
    /// `PendingRequest`.
    pub(crate) fn from_resp_metadata(
        mut metadata: FastlyResponseMetadata,
        error: SendErrorCause,
    ) -> Self {
        let sent_req = metadata.take_sent_req().expect("sent_req must be present");
        let backend_name = metadata.backend().expect("backend must be present").name();
        Self::new(backend_name, sent_req, error)
    }

    /// Create a `SendError` from a `PendingRequest` and an underlying error.
    fn from_pending_req(pending_req: PendingRequest, error: SendErrorCause) -> Self {
        Self::from_resp_metadata(pending_req.metadata, error)
    }

    /// Get the name of the backend that returned this error.
    pub fn backend_name(&self) -> &str {
        self.backend.as_str()
    }

    /// Get the underlying cause of this `SendError`.
    ///
    /// This is the same cause that would be returned by `err.source().downcast_ref::<SendErrorCause>()`, but more direct.
    pub fn root_cause(&self) -> &SendErrorCause {
        &self.error
    }

    /// Convert the error back into the request that was originally sent.
    ///
    /// Since the original request's body is consumed by sending it, the body in the returned
    /// request is empty. To add a new body to the request, use [`Request::with_body()`], for example:
    ///
    /// ```no_run
    /// # use fastly::{Body, Error, Request};
    /// # fn f(bereq: Request) -> Result<(), Error> {
    /// if let Err(e) = bereq.send("my_backend") {
    ///     let new_body = Body::from("something new");
    ///     let new_req = e.into_sent_req().with_body(new_body);
    ///     new_req.send("my_other_backend")?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn into_sent_req(self) -> Request {
        self.sent_req
    }
}

/// Check whether a request looks suitable for sending to a backend.
///
/// Note that this is *not* meant to be a filter for things that could cause security issues, it is
/// only meant to catch errors before the hostcalls do in order to yield friendlier error messages.
fn validate_request(req: &Request) -> Result<(), Error> {
    let scheme_ok = req.url.scheme().eq_ignore_ascii_case("http")
        || req.url.scheme().eq_ignore_ascii_case("https");
    ensure!(
        scheme_ok && req.url.has_authority(),
        "request URIs must have a scheme (http/https) and an authority (host)"
    );
    Ok(())
}
