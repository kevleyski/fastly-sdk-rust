//! HTTP responses.

pub(crate) mod handle;

pub(crate) use self::handle::handles_to_response;

use self::handle::ResponseHandle;
use super::body::{self, Body, StreamingBody};
use super::Request;
use crate::backend::Backend;
use crate::convert::{Borrowable, ToHeaderName, ToHeaderValue, ToStatusCode};
use crate::error::BufferSizeError;
use crate::handle::BodyHandle;
use crate::limits;
use fastly_shared::{FramingHeadersMode, HttpKeepaliveMode};
use http::header::{self, HeaderMap, HeaderName, HeaderValue};
use http::{StatusCode, Version};
use mime::Mime;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::borrow::Cow;
use std::io::BufRead;

/// An HTTP response, including body, headers, and status code.
///
/// # Sending to the client
///
/// Each execution of a Compute@Edge program may send a single response back to the client:
///
/// - [`Response::send_to_client()`]
/// - [`Response::stream_to_client()`]
///
/// If no response is explicitly sent by the program, a default `200 OK` response is sent.
///
/// # Creation and conversion
///
/// Responses can be created programmatically:
///
/// - [`Response::new()`]
/// - [`Response::from_body()`]
/// - [`Response::from_status()`]
///
/// Responses are also returned from backend requests:
///
/// - [`Request::send()`]
/// - [`Request::send_async()`]
/// - [`Request::send_async_streaming()`]
///
/// For interoperability with other Rust libraries, [`Response`] can be converted to and from the
/// [`http`] crate's [`http::Response`] type using the [`From`][`Response::from()`] and
/// [`Into`][`Response::into()`] traits.
///
/// # Builder-style methods
///
/// [`Response`] can be used as a
/// [builder](https://doc.rust-lang.org/1.0.0/style/ownership/builders.html), allowing responses to
/// be constructed and used through method chaining. Methods with the `with_` name prefix, such as
/// [`with_header()`][`Self::with_header()`], return `Self` to allow chaining. The builder style is
/// typically most useful when constructing and using a response in a single expression. For
/// example:
///
/// ```no_run
/// # use fastly::Response;
/// Response::new()
///     .with_header("my-header", "hello!")
///     .with_header("my-other-header", "Здравствуйте!")
///     .send_to_client();
/// ```
///
/// # Setter methods
///
/// Setter methods, such as [`set_header()`][`Self::set_header()`], are prefixed by `set_`, and can
/// be used interchangeably with the builder-style methods, allowing you to mix and match styles
/// based on what is most convenient for your program. Setter methods tend to work better than
/// builder-style methods when constructing a value involves conditional branches or loops. For
/// example:
///
/// ```no_run
/// # use fastly::Response;
/// # let needs_translation = true;
/// let mut resp = Response::new().with_header("my-header", "hello!");
/// if needs_translation {
///     resp.set_header("my-other-header", "Здравствуйте!");
/// }
/// resp.send_to_client();
/// ```
#[derive(Debug)]
pub struct Response {
    version: Version,
    status: StatusCode,
    headers: HeaderMap,
    body: Option<Body>,
    fastly_metadata: Option<FastlyResponseMetadata>,
    framing_headers_mode: FramingHeadersMode,
    http_keepalive_mode: HttpKeepaliveMode,
}

impl Response {
    /// Create a new [`Response`].
    ///
    /// The new response is created with status code `200 OK`, no headers, and an empty body.
    pub fn new() -> Self {
        Self {
            version: Version::HTTP_11,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: None,
            fastly_metadata: None,
            framing_headers_mode: FramingHeadersMode::Automatic,
            http_keepalive_mode: HttpKeepaliveMode::Automatic,
        }
    }

    /// Return whether the response is from a backend request.
    pub fn is_from_backend(&self) -> bool {
        self.fastly_metadata.is_some()
    }

    /// Make a new response with the same headers, status, and version of this response, but no
    /// body.
    ///
    /// If you also need to clone the response body, use
    /// [`clone_with_body()`][`Self::clone_with_body()`]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let original = Response::from_body("hello")
    ///     .with_header("hello", "world!")
    ///     .with_status(418);
    /// let new = original.clone_without_body();
    /// assert_eq!(original.get_header("hello"), new.get_header("hello"));
    /// assert_eq!(original.get_status(), new.get_status());
    /// assert_eq!(original.get_version(), new.get_version());
    /// assert!(original.has_body());
    /// assert!(!new.has_body());
    /// ```
    pub fn clone_without_body(&self) -> Response {
        Self {
            version: self.version,
            status: self.status,
            headers: self.headers.clone(),
            body: None,
            fastly_metadata: self.fastly_metadata.clone(),
            framing_headers_mode: self.framing_headers_mode,
            http_keepalive_mode: self.http_keepalive_mode,
        }
    }

    /// Clone this response by reading in its body, and then writing the same body to the original
    /// and the cloned response.
    ///
    /// This method requires mutable access to this response because reading from and writing to the
    /// body can involve an HTTP connection.
    ///
    #[doc = include_str!("../../docs/snippets/clones-body.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let mut original = Response::from_body("hello")
    ///     .with_header("hello", "world!")
    ///     .with_status(418);
    /// let mut new = original.clone_with_body();
    /// assert_eq!(original.get_header("hello"), new.get_header("hello"));
    /// assert_eq!(original.get_status(), new.get_status());
    /// assert_eq!(original.get_version(), new.get_version());
    /// assert_eq!(original.take_body_bytes(), new.take_body_bytes());
    /// ```
    pub fn clone_with_body(&mut self) -> Response {
        let mut new_resp = self.clone_without_body();
        if self.has_body() {
            for chunk in self.take_body().read_chunks(4096) {
                let chunk = chunk.expect("can read body chunk");
                new_resp.get_body_mut().write_bytes(&chunk);
                self.get_body_mut().write_bytes(&chunk);
            }
        }
        new_resp
    }

    /// Create a new [`Response`] with the given value as the body.
    ///
    #[doc = include_str!("../../docs/snippets/body-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let resp = Response::from_body("hello");
    /// assert_eq!(&resp.into_body_str(), "hello");
    /// ```
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let body_bytes: &[u8] = &[1, 2, 3];
    /// let resp = Response::from_body(body_bytes);
    /// assert_eq!(resp.into_body_bytes().as_slice(), body_bytes);
    /// ```
    pub fn from_body(body: impl Into<Body>) -> Self {
        Self::new().with_body(body)
    }

    /// Create a new response with the given status code.
    ///
    #[doc = include_str!("../../docs/snippets/body-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// use fastly::http::StatusCode;
    /// let resp = Response::from_status(StatusCode::NOT_FOUND);
    /// assert_eq!(resp.get_status().as_u16(), 404);
    /// ```
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// use fastly::http::StatusCode;
    /// let resp = Response::from_status(404);
    /// assert_eq!(resp.get_status(), StatusCode::NOT_FOUND);
    /// ```
    pub fn from_status(status: impl ToStatusCode) -> Self {
        Self::new().with_status(status)
    }

    /// Create a 303 See Other response with the given value as the `Location` header.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// # use http::{header, StatusCode};
    /// let resp = Response::see_other("https://www.fastly.com");
    /// assert_eq!(resp.get_status(), StatusCode::SEE_OTHER);
    /// assert_eq!(resp.get_header_str(header::LOCATION).unwrap(), "https://www.fastly.com");
    /// ```
    pub fn see_other(destination: impl ToHeaderValue) -> Self {
        Self::new()
            .with_status(StatusCode::SEE_OTHER)
            .with_header(header::LOCATION, destination)
    }

    /// Create a 308 Permanent Redirect response with the given value as the `Location` header.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// # use http::{header, StatusCode};
    /// let resp = Response::redirect("https://www.fastly.com");
    /// assert_eq!(resp.get_status(), StatusCode::PERMANENT_REDIRECT);
    /// assert_eq!(resp.get_header_str(header::LOCATION).unwrap(), "https://www.fastly.com");
    /// ```
    pub fn redirect(destination: impl ToHeaderValue) -> Self {
        Self::new()
            .with_status(StatusCode::PERMANENT_REDIRECT)
            .with_header(header::LOCATION, destination)
    }

    /// Create a 307 Temporary Redirect response with the given value as the `Location` header.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// # use http::{header, StatusCode};
    /// let resp = Response::temporary_redirect("https://www.fastly.com");
    /// assert_eq!(resp.get_status(), StatusCode::TEMPORARY_REDIRECT);
    /// assert_eq!(resp.get_header_str(header::LOCATION).unwrap(), "https://www.fastly.com");
    /// ```
    pub fn temporary_redirect(destination: impl ToHeaderValue) -> Self {
        Self::new()
            .with_status(StatusCode::TEMPORARY_REDIRECT)
            .with_header(header::LOCATION, destination)
    }

    /// Builder-style equivalent of [`set_body()`][`Self::set_body()`].
    pub fn with_body(mut self, body: impl Into<Body>) -> Self {
        self.set_body(body);
        self
    }

    /// Returns `true` if this response has a body.
    pub fn has_body(&self) -> bool {
        self.body.is_some()
    }

    /// Get a mutable reference to the body of this response.
    ///
    #[doc = include_str!("../../docs/snippets/creates-empty-body.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// use std::io::Write;
    ///
    /// let mut resp = Response::from_body("hello,");
    /// write!(resp.get_body_mut(), " world!").unwrap();
    /// assert_eq!(&resp.into_body_str(), "hello, world!");
    /// ```
    pub fn get_body_mut(&mut self) -> &mut Body {
        self.body.get_or_insert_with(|| Body::new())
    }

    /// Get a shared reference to the body of this response if it has one, otherwise return `None`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// use std::io::Write;
    ///
    /// let mut resp = Response::new();
    /// assert!(resp.try_get_body_mut().is_none());
    ///
    /// resp.set_body("hello,");
    /// write!(resp.try_get_body_mut().expect("body now exists"), " world!").unwrap();
    /// assert_eq!(&resp.into_body_str(), "hello, world!");
    /// ```
    pub fn try_get_body_mut(&mut self) -> Option<&mut Body> {
        self.body.as_mut()
    }

    /// Get a prefix of this response's body containing up to the given number of bytes.
    ///
    /// See [`Body::get_prefix_mut()`] for details.
    pub fn get_body_prefix_mut(&mut self, length: usize) -> body::Prefix {
        self.get_body_mut().get_prefix_mut(length)
    }

    /// Get a prefix of this response's body as a string containing up to the given number of bytes.
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

    /// Set the given value as the response's body.
    #[doc = include_str!("../../docs/snippets/body-argument.md")]
    #[doc = include_str!("../../docs/snippets/discards-body.md")]
    pub fn set_body(&mut self, body: impl Into<Body>) {
        self.body = Some(body.into());
    }

    /// Take and return the body from this response.
    ///
    /// After calling this method, this response will no longer have a body.
    ///
    #[doc = include_str!("../../docs/snippets/creates-empty-body.md")]
    pub fn take_body(&mut self) -> Body {
        self.body.take().unwrap_or_else(|| Body::new())
    }

    /// Take and return the body from this response if it has one, otherwise return `None`.
    ///
    /// After calling this method, this response will no longer have a body.
    pub fn try_take_body(&mut self) -> Option<Body> {
        self.body.take()
    }

    /// Append another [`Body`] to the body of this response without reading or writing any body
    /// contents.
    ///
    /// If this response does not have a body, the appended body is set as the response's body.
    ///
    #[doc = include_str!("../../docs/snippets/body-append-constant-time.md")]
    ///
    /// This method should be used when combining bodies that have not necessarily been read yet,
    /// such as a body returned from a backend response. To append contents that are already in
    /// memory as strings or bytes, use [`get_body_mut()`][`Self::get_body_mut()`] to write the
    /// contents to the end of the body.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::{Request, Response};
    /// let mut resp = Response::from_body("hello! backend says: ");
    /// let backend_resp = Request::get("https://example.com/").send("example_backend").unwrap();
    /// resp.append_body(backend_resp.into_body());
    /// resp.send_to_client();
    /// ```
    pub fn append_body(&mut self, other: Body) {
        if let Some(ref mut body) = &mut self.body {
            body.append(other);
        } else {
            self.body = Some(other);
        }
    }

    /// Consume the response and return its body as a byte vector.
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body-reqresp.md")]
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let resp = Response::from_body(b"hello, world!".to_vec());
    /// let bytes = resp.into_body_bytes();
    /// assert_eq!(&bytes, b"hello, world!");
    pub fn into_body_bytes(mut self) -> Vec<u8> {
        self.take_body_bytes()
    }

    /// Consume the response and return its body as a string.
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
    /// # use fastly::Response;
    /// let resp = Response::from_body("hello, world!");
    /// let string = resp.into_body_str();
    /// assert_eq!(&string, "hello, world!");
    /// ```
    pub fn into_body_str(mut self) -> String {
        self.take_body_str()
    }

    /// Consume the response and return its body as a string, including invalid characters.
    ///
    #[doc = include_str!("../../docs/snippets/utf8-replacement.md")]
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body-reqresp.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let mut resp = Response::new();
    /// resp.set_body_octet_stream(b"\xF0\x90\x80 hello, world!");
    /// let string = resp.into_body_str_lossy();
    /// assert_eq!(&string, "� hello, world!");
    /// ```
    pub fn into_body_str_lossy(mut self) -> String {
        self.take_body_str_lossy()
    }

    /// Consume the response and return its body.
    ///
    #[doc = include_str!("../../docs/snippets/creates-empty-body.md")]
    pub fn into_body(self) -> Body {
        self.body.unwrap_or_else(|| Body::new())
    }

    /// Consume the response and return its body if it has one, otherwise return `None`.
    pub fn try_into_body(self) -> Option<Body> {
        self.body
    }

    /// Builder-style equivalent of [`set_body_text_plain()`][`Self::set_body_text_plain()`].
    pub fn with_body_text_plain(mut self, body: &str) -> Self {
        self.set_body_text_plain(body);
        self
    }

    /// Set the given string as the response's body with content type `text/plain; charset=UTF-8`.
    ///
    #[doc = include_str!("../../docs/snippets/discards-body.md")]
    #[doc = include_str!("../../docs/snippets/sets-text-plain.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let mut resp = Response::new();
    /// resp.set_body_text_plain("hello, world!");
    /// assert_eq!(resp.get_content_type(), Some(fastly::mime::TEXT_PLAIN_UTF_8));
    /// assert_eq!(&resp.into_body_str(), "hello, world!");
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
    /// # use fastly::Response;
    /// let mut resp = Response::new();
    /// resp.set_body_text_html("<p>hello, world!</p>");
    /// assert_eq!(resp.get_content_type(), Some(fastly::mime::TEXT_HTML_UTF_8));
    /// assert_eq!(&resp.into_body_str(), "<p>hello, world!</p>");
    /// ```
    pub fn set_body_text_html(&mut self, body: &str) {
        self.body = Some(Body::from(body));
        self.set_content_type(mime::TEXT_HTML_UTF_8);
    }

    /// Take and return the body from this response as a string.
    ///
    /// After calling this method, this response will no longer have a body.
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
    /// # use fastly::Response;
    /// let mut resp = Response::from_body("hello, world!");
    /// let string = resp.take_body_str();
    /// assert!(resp.try_take_body().is_none());
    /// assert_eq!(&string, "hello, world!");
    /// ```
    pub fn take_body_str(&mut self) -> String {
        if let Some(body) = self.try_take_body() {
            body.into_string()
        } else {
            String::new()
        }
    }

    /// Take and return the body from this response as a string, including invalid characters.
    ///
    #[doc = include_str!("../../docs/snippets/utf8-replacement.md")]
    ///
    /// After calling this method, this response will no longer have a body.
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body-reqresp.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let mut resp = Response::new();
    /// resp.set_body_octet_stream(b"\xF0\x90\x80 hello, world!");
    /// let string = resp.take_body_str_lossy();
    /// assert!(resp.try_take_body().is_none());
    /// assert_eq!(&string, "� hello, world!");
    /// ```
    pub fn take_body_str_lossy(&mut self) -> String {
        if let Some(body) = self.try_take_body() {
            String::from_utf8_lossy(&body.into_bytes()).to_string()
        } else {
            String::new()
        }
    }

    /// Return a [`Lines`][`std::io::Lines`] iterator that reads the response body a line at a time.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::{Body, Response};
    /// use std::io::Write;
    ///
    /// fn remove_es(resp: &mut Response) {
    ///     let mut no_es = Body::new();
    ///     for line in resp.read_body_lines() {
    ///         writeln!(no_es, "{}", line.unwrap().replace("e", "")).unwrap();
    ///     }
    ///     resp.set_body(no_es);
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

    /// Set the given bytes as the response's body.
    ///
    #[doc = include_str!("../../docs/snippets/discards-body.md")]
    #[doc = include_str!("../../docs/snippets/sets-app-octet-stream.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let mut resp = Response::new();
    /// resp.set_body_octet_stream(b"hello, world!");
    /// assert_eq!(resp.get_content_type(), Some(fastly::mime::APPLICATION_OCTET_STREAM));
    /// assert_eq!(&resp.into_body_bytes(), b"hello, world!");
    /// ```
    pub fn set_body_octet_stream(&mut self, body: &[u8]) {
        self.body = Some(Body::from(body));
        self.set_content_type(mime::APPLICATION_OCTET_STREAM);
    }

    /// Take and return the body from this response as a string.
    ///
    /// After calling this method, this response will no longer have a body.
    ///
    #[doc = include_str!("../../docs/snippets/buffers-body-reqresp.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let mut resp = Response::from_body(b"hello, world!".to_vec());
    /// let bytes = resp.take_body_bytes();
    /// assert!(resp.try_take_body().is_none());
    /// assert_eq!(&bytes, b"hello, world!");
    /// ```
    pub fn take_body_bytes(&mut self) -> Vec<u8> {
        if let Some(body) = self.try_take_body() {
            body.into_bytes()
        } else {
            Vec::new()
        }
    }

    /// Return an iterator that reads the response body in chunks of at most the given number of
    /// bytes.
    ///
    /// If `chunk_size` does not evenly divide the length of the body, then the last chunk will not
    /// have length `chunk_size`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::{Body, Response};
    /// fn remove_0s(resp: &mut Response) {
    ///     let mut no_0s = Body::new();
    ///     for chunk in resp.read_body_chunks(4096) {
    ///         let mut chunk = chunk.unwrap();
    ///         chunk.retain(|b| *b != 0);
    ///         no_0s.write_bytes(&chunk);
    ///     }
    ///     resp.set_body(no_0s);
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

    /// Convert the given value to JSON and set that JSON as the response's body.
    ///
    /// The given value must implement [`serde::Serialize`]. You can either implement that trait for
    /// your own custom type, or use [`serde_json::Value`] to create untyped JSON values. See
    /// [`serde_json`] for details.
    ///
    #[doc = include_str!("../../docs/snippets/discards-body.md")]
    #[doc = include_str!("../../docs/snippets/sets-app-json.md")]
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
    /// # use fastly::Response;
    /// #[derive(serde::Serialize)]
    /// struct MyData {
    ///     name: String,
    ///     count: u64,
    /// }
    /// let my_data = MyData { name: "Computers".to_string(), count: 1024 };
    /// let mut resp = Response::new();
    /// resp.set_body_json(&my_data).unwrap();
    /// assert_eq!(resp.get_content_type(), Some(fastly::mime::APPLICATION_JSON));
    /// assert_eq!(&resp.into_body_str(), r#"{"name":"Computers","count":1024}"#);
    /// ```
    ///
    /// Using untyped JSON and the [`serde_json::json`] macro:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let my_data = serde_json::json!({
    ///     "name": "Computers",
    ///     "count": 1024,
    /// });
    /// let mut resp = Response::new();
    /// resp.set_body_json(&my_data).unwrap();
    /// assert_eq!(resp.get_content_type(), Some(fastly::mime::APPLICATION_JSON));
    /// assert_eq!(&resp.into_body_str(), r#"{"count":1024,"name":"Computers"}"#);
    /// ```
    pub fn set_body_json(&mut self, value: &impl Serialize) -> Result<(), serde_json::Error> {
        self.body = Some(Body::new());
        serde_json::to_writer(self.get_body_mut(), value)?;
        self.set_content_type(mime::APPLICATION_JSON);
        Ok(())
    }

    /// Take the response body and attempt to parse it as a JSON value.
    ///
    /// The return type must implement [`serde::Deserialize`] without any non-static lifetimes. You
    /// can either implement that trait for your own custom type, or use [`serde_json::Value`] to
    /// deserialize untyped JSON values. See [`serde_json`] for details.
    ///
    /// After calling this method, this response will no longer have a body.
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
    /// # use fastly::Response;
    /// #[derive(serde::Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     count: u64,
    /// }
    /// let mut resp = Response::from_body(r#"{"name":"Computers","count":1024}"#);
    /// let my_data = resp.take_body_json::<MyData>().unwrap();
    /// assert_eq!(&my_data.name, "Computers");
    /// assert_eq!(my_data.count, 1024);
    /// ```
    ///
    /// Using untyped JSON with [`serde_json::Value`]:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let my_data = serde_json::json!({
    ///     "name": "Computers",
    ///     "count": 1024,
    /// });
    /// let mut resp = Response::from_body(r#"{"name":"Computers","count":1024}"#);
    /// let my_data = resp.take_body_json::<serde_json::Value>().unwrap();
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
    /// the response's body.
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
    /// # use fastly::Response;
    /// #[derive(serde::Serialize)]
    /// struct MyData {
    ///     name: String,
    ///     count: u64,
    /// }
    /// let my_data = MyData { name: "Computers".to_string(), count: 1024 };
    /// let mut resp = Response::new();
    /// resp.set_body_form(&my_data).unwrap();
    /// assert_eq!(resp.get_content_type(), Some(fastly::mime::APPLICATION_WWW_FORM_URLENCODED));
    /// assert_eq!(&resp.into_body_str(), "name=Computers&count=1024");
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

    /// Take the response body and attempt to parse it as a `application/x-www-form-urlencoded`
    /// formatted string.
    ///
    #[doc = include_str!("../../docs/snippets/returns-deserializeowned.md")]
    ///
    /// After calling this method, this response will no longer have a body.
    ///
    /// # Errors
    ///
    /// This method returns [`serde_urlencoded::de::Error`] if deserialization fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// #[derive(serde::Deserialize)]
    /// struct MyData {
    ///     name: String,
    ///     count: u64,
    /// }
    /// let mut resp = Response::from_body("name=Computers&count=1024");
    /// let my_data = resp.take_body_form::<MyData>().unwrap();
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

    /// Get the MIME type described by the response's
    /// [`Content-Type`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Type)
    /// header, or `None` if that header is absent or contains an invalid MIME type.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let resp = Response::new().with_body_text_plain("hello, world!");
    /// assert_eq!(resp.get_content_type(), Some(fastly::mime::TEXT_PLAIN_UTF_8));
    /// ```
    pub fn get_content_type(&self) -> Option<Mime> {
        self.get_header_str(http::header::CONTENT_TYPE)
            .and_then(|v| v.parse().ok())
    }

    /// Builder-style equivalent of [`set_content_type()`][`Self::set_content_type()`].
    pub fn with_content_type(mut self, mime: Mime) -> Self {
        self.set_content_type(mime);
        self
    }

    /// Set the MIME type described by the response's
    /// [`Content-Type`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Type)
    /// header.
    ///
    /// Any existing `Content-Type` header values will be overwritten.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let mut resp = Response::new().with_body("hello,world!");
    /// resp.set_content_type(fastly::mime::TEXT_CSV_UTF_8);
    /// ```
    pub fn set_content_type(&mut self, mime: Mime) {
        self.set_header(http::header::CONTENT_TYPE, mime.as_ref())
    }

    /// Get the value of the response's
    /// [`Content-Length`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Length)
    /// header, if it exists.
    pub fn get_content_length(&self) -> Option<usize> {
        self.get_header(http::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
    }

    /// Returns whether the given header name is present in the response.
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let resp = Response::new().with_header("hello", "world!");
    /// assert!(resp.contains_header("hello"));
    /// assert!(!resp.contains_header("not-present"));
    /// ```
    pub fn contains_header(&self, name: impl ToHeaderName) -> bool {
        self.headers.contains_key(name.into_borrowable().as_ref())
    }

    /// Builder-style equivalent of [`set_header()`][`Self::set_header()`].
    pub fn with_header(mut self, name: impl ToHeaderName, value: impl ToHeaderValue) -> Self {
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
    /// # use fastly::Response;
    /// let resp = Response::new().with_header("hello", "world!");
    /// assert_eq!(resp.get_header_str("hello"), Some("world"));
    /// ```
    pub fn get_header_str<'a>(&self, name: impl ToHeaderName) -> Option<&str> {
        let name = name.into_borrowable();
        if let Some(hdr) = self.get_header(name.as_ref()) {
            Some(
                hdr.to_str().unwrap_or_else(|_| {
                    panic!("invalid UTF-8 HTTP header value for header: {}", name)
                }),
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
    /// # use fastly::Response;
    /// # use http::header::HeaderValue;
    /// # use std::borrow::Cow;
    /// let header_value = HeaderValue::from_bytes(b"\xF0\x90\x80 world!").unwrap();
    /// let resp = Response::new().with_header("hello", header_value);
    /// assert_eq!(resp.get_header_str_lossy("hello"), Some(Cow::from("� world")));
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
    /// # use fastly::Response;
    /// # use fastly::http::HeaderValue;
    /// let resp = Response::new().with_header("hello", "world!");
    /// assert_eq!(resp.get_header("hello"), Some(&HeaderValue::from_static("world")));
    /// ```
    ///
    /// Safely handling invalid UTF-8 values:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let invalid_utf8 = &"🐈".as_bytes()[0..3];
    /// let resp = Response::new().with_header("hello", invalid_utf8);
    /// assert_eq!(resp.get_header("hello").unwrap().as_bytes(), invalid_utf8);
    /// ```
    pub fn get_header<'a>(&self, name: impl ToHeaderName) -> Option<&HeaderValue> {
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
    /// # use fastly::Response;
    /// let resp = Response::new()
    ///     .with_header("hello", "world!")
    ///     .with_header("hello", "universe!");
    /// let values = resp.get_header_all_str("hello");
    /// assert_eq!(values.len(), 2);
    /// assert!(values.contains(&"world!"));
    /// assert!(values.contains(&"universe!"));
    /// ```
    pub fn get_header_all_str<'a>(&self, name: impl ToHeaderName) -> Vec<&str> {
        let name = name.into_borrowable();
        self.get_header_all(name.as_ref())
            .map(|v| {
                v.to_str()
                    .unwrap_or_else(|_| panic!("non-UTF-8 HTTP header value for header: {}", name))
            })
            .collect()
    }

    /// Get an iterator of all the response's header names and values.
    ///
    /// # Examples
    ///
    /// You can turn the iterator into a collection, like [`Vec`]:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// # use fastly::http::header::{HeaderName, HeaderValue};
    /// let resp = Response::new()
    ///     .with_header("hello", "world!")
    ///     .with_header("hello", "universe!");
    ///
    /// let headers: Vec<(&HeaderName, &HeaderValue)> = resp.get_headers().collect();
    /// assert_eq!(headers.len(), 2);
    /// assert!(headers.contains(&(&HeaderName::from_static("hello"), &HeaderValue::from_static("world!"))));
    /// assert!(headers.contains(&(&HeaderName::from_static("hello"), &HeaderValue::from_static("universe!"))));
    /// ```
    ///
    /// You can use the iterator in a loop:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let resp = Response::new()
    ///     .with_header("hello", "world!")
    ///     .with_header("hello", "universe!");
    ///
    /// for (n, v) in resp.get_headers() {
    ///     println!("Header -  {}: {:?}", n, v);
    /// }
    /// ```
    pub fn get_headers(&self) -> impl Iterator<Item = (&HeaderName, &HeaderValue)> {
        self.headers.iter()
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
    /// # use fastly::Response;
    /// let world_value = HeaderValue::from_bytes(b"\xF0\x90\x80 world!").unwrap();
    /// let universe_value = HeaderValue::from_bytes(b"\xF0\x90\x80 universe!").unwrap();
    /// let resp = Response::new()
    ///     .with_header("hello", world_value)
    ///     .with_header("hello", universe_value);
    /// let values = resp.get_header_all_str_lossy("hello");
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
    /// # use fastly::Response;
    /// # use fastly::http::HeaderValue;
    /// let invalid_utf8 = &"🐈".as_bytes()[0..3];
    /// let resp = Response::new()
    ///     .with_header("hello", "world!")
    ///     .with_header("hello", invalid_utf8);
    ///
    /// let values: Vec<&HeaderValue> = resp.get_header_all("hello").collect();
    /// assert_eq!(values.len(), 2);
    /// assert!(values.contains(&&HeaderValue::from_static("world!")));
    /// assert!(values.contains(&&HeaderValue::from_bytes(invalid_utf8).unwrap()));
    /// ```
    ///
    /// You can use the iterator in a loop:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let invalid_utf8 = &"🐈".as_bytes()[0..3];
    /// let resp = Response::new()
    ///     .with_header("hello", "world!")
    ///     .with_header("hello", invalid_utf8);
    ///
    /// for value in resp.get_header_all("hello") {
    ///     if let Ok(s) = value.to_str() {
    ///         println!("hello, {}", s);
    ///     } else {
    ///         println!("hello, invalid UTF-8!");
    ///     }
    /// }
    /// ```
    pub fn get_header_all<'a>(
        &'a self,
        name: impl ToHeaderName,
    ) -> impl Iterator<Item = &'a HeaderValue> {
        self.headers.get_all(name.into_borrowable().as_ref()).iter()
    }

    /// Get all of the response's header names as strings, or an empty vector if no headers are
    /// present.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let resp = Response::new()
    ///     .with_header("hello", "world!")
    ///     .with_header("goodbye", "latency!");
    /// let names = resp.get_header_names_str();
    /// assert_eq!(names.len(), 2);
    /// assert!(names.contains(&"hello"));
    /// assert!(names.contains(&"goodbye"));
    /// ```
    pub fn get_header_names_str(&self) -> Vec<&str> {
        self.get_header_names().map(|n| n.as_str()).collect()
    }

    /// Get an iterator of all the response's header names.
    ///
    /// # Examples
    ///
    /// You can turn the iterator into a collection, like [`Vec`]:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// # use fastly::http::header::HeaderName;
    /// let resp = Response::new()
    ///     .with_header("hello", "world!")
    ///     .with_header("goodbye", "latency!");
    ///
    /// let values: Vec<&HeaderName> = resp.get_header_names().collect();
    /// assert_eq!(values.len(), 2);
    /// assert!(values.contains(&&HeaderName::from_static("hello")));
    /// assert!(values.contains(&&HeaderName::from_static("goodbye")));
    /// ```
    ///
    /// You can use the iterator in a loop:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let resp = Response::new()
    ///     .with_header("hello", "world!")
    ///     .with_header("goodbye", "latency!");
    ///
    /// for name in resp.get_header_names() {
    ///     println!("saw header: {:?}", name);
    /// }
    /// ```
    pub fn get_header_names(&self) -> impl Iterator<Item = &HeaderName> {
        self.headers.keys()
    }

    /// Set a response header to the given value, discarding any previous values for the given
    /// header name.
    ///
    #[doc = include_str!("../../docs/snippets/header-name-value-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let mut resp = Response::new();
    ///
    /// resp.set_header("hello", "world!");
    /// assert_eq!(resp.get_header_str("hello"), Some("world!"));
    ///
    /// resp.set_header("hello", "universe!");
    ///
    /// let values = resp.get_header_all_str("hello");
    /// assert_eq!(values.len(), 1);
    /// assert!(!values.contains(&"world!"));
    /// assert!(values.contains(&"universe!"));
    /// ```
    pub fn set_header(&mut self, name: impl ToHeaderName, value: impl ToHeaderValue) {
        self.headers.insert(name.into_owned(), value.into_owned());
    }

    /// Add a response header with given value.
    ///
    /// Unlike [`set_header()`][`Self::set_header()`], this does not discard existing values for the
    /// same header name.
    ///
    #[doc = include_str!("../../docs/snippets/header-name-value-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let mut resp = Response::new();
    ///
    /// resp.set_header("hello", "world!");
    /// assert_eq!(resp.get_header_str("hello"), Some("world!"));
    ///
    /// resp.append_header("hello", "universe!");
    ///
    /// let values = resp.get_header_all_str("hello");
    /// assert_eq!(values.len(), 2);
    /// assert!(values.contains(&"world!"));
    /// assert!(values.contains(&"universe!"));
    /// ```
    pub fn append_header(&mut self, name: impl ToHeaderName, value: impl ToHeaderValue) {
        self.headers
            .append(name.into_borrowable().as_ref(), value.into_owned());
    }

    /// Remove all response headers of the given name, and return one of the removed header values
    /// if any were present.
    ///
    /// If the header has multiple values, one is returned arbitrarily. To get all of the removed
    /// header values, or to get a specific value, use
    /// [`get_header_all()`][`Self::get_header_all()`].
    ///
    #[doc = include_str!("../../docs/snippets/header-name-argument.md")]
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// # use fastly::http::HeaderValue;
    /// let mut resp = Response::new().with_header("hello", "world!");
    /// assert_eq!(resp.get_header_str("hello"), Some("world!"));
    /// assert_eq!(resp.remove_header("hello"), Some(HeaderValue::from_static("world!")));
    /// assert!(resp.remove_header("not-present").is_none());
    /// ```
    pub fn remove_header(&mut self, name: impl ToHeaderName) -> Option<HeaderValue> {
        self.headers.remove(name.into_borrowable().as_ref())
    }

    /// Remove all response headers of the given name, and return one of the removed header values
    /// as a string if any were present.
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
    /// # use fastly::Response;
    /// let mut resp = Response::new().with_header("hello", "world!");
    /// assert_eq!(resp.get_header_str("hello"), Some("world!"));
    /// assert_eq!(resp.remove_header_str("hello"), Some("world!".to_string()));
    /// assert!(resp.remove_header_str("not-present").is_none());
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

    /// Remove all response headers of the given name, and return one of the removed header values
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
    /// # use fastly::Response;
    /// # use http::header::HeaderValue;
    /// # use std::borrow::Cow;
    /// let header_value = HeaderValue::from_bytes(b"\xF0\x90\x80 world!").unwrap();
    /// let mut resp = Response::new().with_header("hello", header_value);
    /// assert_eq!(resp.get_header_str_lossy("hello"), Some(Cow::from("� world")));
    /// assert_eq!(resp.remove_header_str_lossy("hello"), Some(String::from("� world")));
    /// assert!(resp.remove_header_str_lossy("not-present").is_none());
    /// ```
    pub fn remove_header_str_lossy(&mut self, name: impl ToHeaderName) -> Option<String> {
        self.remove_header(name)
            .map(|hdr| String::from_utf8_lossy(hdr.as_bytes()).into_owned())
    }

    /// Builder-style equivalent of [`set_status()`][`Self::set_status()`].
    pub fn with_status(mut self, status: impl ToStatusCode) -> Self {
        self.set_status(status);
        self
    }

    /// Get the HTTP status code of the response.
    pub fn get_status(&self) -> StatusCode {
        self.status
    }

    /// Set the HTTP status code of the response.
    ///
    #[doc = include_str!("../../docs/snippets/statuscode-argument.md")]
    ///
    /// # Examples
    ///
    /// Using the constants from [`StatusCode`]:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// use fastly::http::StatusCode;
    ///
    /// let mut resp = Response::from_body("not found!");
    /// resp.set_status(StatusCode::NOT_FOUND);
    /// resp.send_to_client();
    /// ```
    ///
    /// Using a `u16`:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let mut resp = Response::from_body("not found!");
    /// resp.set_status(404);
    /// resp.send_to_client();
    /// ```
    pub fn set_status(&mut self, status: impl ToStatusCode) {
        self.status = status.to_status_code();
    }

    /// Builder-style equivalent of [`set_version()`][`Self::set_version()`].
    pub fn with_version(mut self, version: Version) -> Self {
        self.set_version(version);
        self
    }

    /// Get the HTTP version of this response.
    pub fn get_version(&self) -> Version {
        self.version
    }

    /// Set the HTTP version of this response.
    pub fn set_version(&mut self, version: Version) {
        self.version = version;
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

    /// Sets whether the client is encouraged to stop using the current connection and to open
    /// a new one for the next request.
    ///
    /// See [`HttpKeepaliveMode`] for details on the options.
    ///
    /// While this method never fails, Compute@Edge may not always respect the specificed
    /// [`HttpKeepaliveMode`]. In those cases, the `Response` being configured will otherwise be
    /// intact, and Compute@Edge will default to [`HttpKeepaliveMode::Automatic`].
    #[doc(hidden)]
    pub fn set_http_keepalive_mode(&mut self, mode: HttpKeepaliveMode) {
        self.http_keepalive_mode = mode;
    }

    /// Builder-style equivalent of
    /// [`set_http_keepalive_mode()`][`Self::set_http_keepalive_mode()`].
    #[doc(hidden)]
    pub fn with_http_keepalive_mode(mut self, mode: HttpKeepaliveMode) -> Self {
        self.set_http_keepalive_mode(mode);
        self
    }

    /// Get the name of the [`Backend`] this response came from, or `None` if the response is
    /// synthetic.
    ///
    /// # Examples
    ///
    /// From a backend response:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let backend_resp = Request::get("https://example.com/").send("example_backend").unwrap();
    /// assert_eq!(backend_resp.get_backend_name(), Some("example_backend"));
    /// ```
    ///
    /// From a synthetic response:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let synthetic_resp = Response::new();
    /// assert!(synthetic_resp.get_backend_name().is_none());
    /// ```
    pub fn get_backend_name(&self) -> Option<&str> {
        self.get_backend().map(|be| be.name())
    }

    /// Get the backend this response came from, or `None` if the response is synthetic.
    ///
    /// # Examples
    ///
    /// From a backend response:
    ///
    /// ```no_run
    /// # use fastly::{Backend, Request};
    /// let backend_resp = Request::get("https://example.com/").send("example_backend").unwrap();
    /// assert_eq!(backend_resp.get_backend(), Some(&Backend::from_name("example_backend").unwrap()));
    /// ```
    ///
    /// From a synthetic response:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let synthetic_resp = Response::new();
    /// assert!(synthetic_resp.get_backend().is_none());
    /// ```
    pub fn get_backend(&self) -> Option<&Backend> {
        self.fastly_metadata.as_ref().and_then(|md| md.backend())
    }

    /// Get the request this response came from, or `None` if the response is synthetic.
    ///
    /// Note that the returned request will only have the headers and metadata of the original
    /// request, as the body is consumed when sending the request.
    ///
    /// This method only returns a reference to the backend request. To instead take and return the
    /// owned request (for example, to subsequently send the request again), use
    /// [`take_backend_request()`][`Self::take_backend_request()`].
    ///
    /// # Examples
    ///
    /// From a backend response:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let backend_resp = Request::post("https://example.com/")
    ///     .with_body("hello")
    ///     .send("example_backend")
    ///     .unwrap();
    /// let backend_req = backend_resp.get_backend_request().expect("response is not synthetic");
    /// assert_eq!(backend_req.get_url_str(), "https://example.com/");
    /// assert!(!backend_req.has_body());
    /// ```
    ///
    /// From a synthetic response:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let synthetic_resp = Response::new();
    /// assert!(synthetic_resp.get_backend_request().is_none());
    /// ```
    pub fn get_backend_request(&self) -> Option<&Request> {
        self.fastly_metadata.as_ref().and_then(|md| md.sent_req())
    }

    /// Take and return the request this response came from, or `None` if the response is synthetic.
    ///
    /// Note that the returned request will only have the headers and metadata of the original
    /// request, as the body is consumed when sending the request.
    ///
    /// # Examples
    ///
    /// From a backend response:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut backend_resp = Request::post("https://example.com/")
    ///     .with_body("hello")
    ///     .send("example_backend")
    ///     .unwrap();
    /// let backend_req = backend_resp.take_backend_request().expect("response is not synthetic");
    /// assert_eq!(backend_req.get_url_str(), "https://example.com/");
    /// assert!(!backend_req.has_body());
    /// backend_req.with_body("goodbye").send("example_backend").unwrap();
    /// ```
    ///
    /// From a synthetic response:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// let mut synthetic_resp = Response::new();
    /// assert!(synthetic_resp.take_backend_request().is_none());
    /// ```
    pub fn take_backend_request(&mut self) -> Option<Request> {
        self.fastly_metadata
            .as_mut()
            .and_then(|md| md.take_sent_req())
    }

    pub(crate) fn set_fastly_metadata(&mut self, md: FastlyResponseMetadata) {
        self.fastly_metadata = Some(md);
    }

    /// Begin sending the response to the client.
    ///
    /// This method returns as soon as the response header begins sending to the client, and
    /// transmission of the response will continue in the background.
    ///
    /// Once this method is called, nothing else may be added to the response body. To stream
    /// additional data to a response body after it begins to send, use
    /// [`stream_to_client`][`Self::stream_to_client()`].
    ///
    /// # Panics
    ///
    /// This method panics if another response has already been sent to the client by this method,
    /// by [`stream_to_client()`][`Self::stream_to_client()`], or by the equivalent methods of
    /// [`ResponseHandle`].
    ///
    #[doc = include_str!("../../docs/snippets/explicit-send-fastly-main.md")]
    ///
    /// # Examples
    ///
    /// Sending a backend response without modification:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// Request::get("https://example.com/").send("example_backend").unwrap().send_to_client();
    /// ```
    ///
    /// Removing a header from a backend response before sending to the client:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// let mut backend_resp = Request::get("https://example.com/").send("example_backend").unwrap();
    /// backend_resp.remove_header("bad-header");
    /// backend_resp.send_to_client();
    /// ```
    ///
    /// Sending a synthetic response:
    ///
    /// ```no_run
    /// # use fastly::Response;
    /// Response::from_body("hello, world!").send_to_client();
    /// ```
    pub fn send_to_client(self) {
        let res = self.send_to_client_impl(false, true);
        debug_assert!(res.is_none());
    }

    /// Begin sending the response to the client, and return a [`StreamingBody`] that can accept
    /// further data to send.
    ///
    /// The client connection must be closed when finished writing the response by calling
    /// [`StreamingBody::finish()`].
    ///
    /// This method is most useful for programs that do some sort of processing or inspection of a
    /// potentially-large backend response body. Streaming allows the program to operate on small
    /// parts of the body rather than having to read it all into memory at once.
    ///
    /// This method returns as soon as the response header begins sending to the client, and
    /// transmission of the response will continue in the background.
    ///
    /// # Panics
    ///
    /// This method panics if another response has already been sent to the client by this method,
    /// by [`send_to_client()`][`Self::send_to_client()`], or by the equivalent methods of
    /// [`ResponseHandle`].
    ///
    #[doc = include_str!("../../docs/snippets/explicit-send-fastly-main.md")]
    ///
    /// # Examples
    ///
    /// Count the number of lines in a UTF-8 backend response body while sending it to the client:
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// use std::io::BufRead;
    ///
    /// let mut backend_resp = Request::get("https://example.com/").send("example_backend").unwrap();
    /// // Take the body so we can iterate through its lines later
    /// let backend_resp_body = backend_resp.take_body();
    /// // Start sending the backend response to the client with a now-empty body
    /// let mut client_body = backend_resp.stream_to_client();
    ///
    /// let mut num_lines = 0;
    /// for line in backend_resp_body.lines() {
    ///     let line = line.unwrap();
    ///     num_lines += 1;
    ///     // Write the line to the streaming client body
    ///     client_body.write_str(&line);
    /// }
    /// // Finish the streaming body to close the client connection
    /// client_body.finish().unwrap();
    ///
    /// println!("backend response body contained {} lines", num_lines);
    /// ```
    pub fn stream_to_client(self) -> StreamingBody {
        let res = self.send_to_client_impl(true, true);
        // streaming = true means we always get back a `Some`
        res.expect("streaming body is present")
    }

    /// Send a response to the client.
    ///
    /// This will return a [`StreamingBody`] if and only if `streaming` is true. If a response has
    /// already been sent to the client, and `panic_on_multiple_send` is `true`, this function will
    /// panic.
    ///
    /// This method is public, but hidden from generated documentation in order to support the
    /// implementation of [`panic_with_status()`].
    #[doc(hidden)]
    pub fn send_to_client_impl(
        self,
        streaming: bool,
        panic_on_multiple_send: bool,
    ) -> Option<StreamingBody> {
        assert_single_downstream_response_is_sent(panic_on_multiple_send);

        let (resp_handle, body_handle) = self.into_handles();

        // Send the response to the client using the appropriate method based on the `streaming` flag.
        if streaming {
            Some(resp_handle.stream_to_client(body_handle).into())
        } else {
            resp_handle.send_to_client(body_handle);
            None
        }
    }

    /// Create a [`Response`] from the a [`ResponseHandle`] and a [`BodyHandle`], returning an error
    /// if any [`ResponseLimits`][`crate::limits::ResponseLimits`] are exceeded.
    ///
    /// The extra metadata associated with a backend response is not tracked by the low-level handle
    /// APIs. As a result, methods like [`get_backend()`][`Self::get_backend()`] and
    /// [`get_backend_request()`][`Self::get_backend_request()`] will always return `None` for a
    /// request created from handles.
    pub fn from_handles(
        resp_handle: ResponseHandle,
        body_handle: BodyHandle,
    ) -> Result<Self, BufferSizeError> {
        let mut resp = Response::new()
            .with_status(resp_handle.get_status())
            .with_version(resp_handle.get_version());
        let resp_limits = limits::RESPONSE_LIMITS.read().unwrap();

        for name in resp_handle.get_header_names_impl(
            limits::DEFAULT_MAX_HEADER_NAME_BYTES,
            resp_limits.max_header_name_bytes,
        ) {
            let name = name?;
            for value in resp_handle.get_header_values_impl(
                &name,
                limits::DEFAULT_MAX_HEADER_VALUE_BYTES,
                resp_limits.max_header_value_bytes,
            ) {
                let value = value?;
                resp.append_header(&name, value);
            }
        }

        Ok(resp.with_body(body_handle))
    }

    /// Create a [`ResponseHandle`]/[`BodyHandle`] pair from a [`Response`].
    ///
    /// The extra metadata associated with a backend response is not tracked by the low-level handle
    /// APIs. As a result, converting to handles will cause the backend and request associated with
    /// a backend response to be lost.
    pub fn into_handles(mut self) -> (ResponseHandle, BodyHandle) {
        // Convert to a body handle, or create an empty body handle if none is set.
        let body_handle = if let Some(body) = self.try_take_body() {
            body.into_handle()
        } else {
            BodyHandle::new()
        };

        // Mint a response handle, and set the HTTP status code, version, and headers.
        let mut resp_handle = ResponseHandle::new();
        resp_handle.set_status(self.status);
        resp_handle.set_version(self.version);
        for name in self.headers.keys() {
            resp_handle.set_header_values(name, self.headers.get_all(name));
        }
        resp_handle.set_framing_headers_mode(self.framing_headers_mode);
        // If we are not permitted to set a keepalive mode on the response, proceed anyway. This
        // does not impede the integrity of a response a client would like to send, and could be
        // for relatively inoccuous reasons such as C@E determining a WASM program may not have
        // useful input on the state of a client connection.
        //
        // Ignoring the error here is how the fallible nature of
        // `Response::set_http_keepalive_mode` is upheld: `ResponseHandle::set_http_keepalive_mode`
        // is fallible on the condition that C@E denies a request to set a particular keepalive
        // mode. If this lower-level call succeeds, C@E will do as promised; it's just that we
        // allow this failure in support of a simpler high-level interface. It's almost certainly
        // overkill to fail response handling just because we'll retain a default posture of
        // keeping a client connection alive.
        let _ = resp_handle.set_http_keepalive_mode(self.http_keepalive_mode);

        (resp_handle, body_handle)
    }
}

/// Anything that we need to make a full roundtrip through the `http` types that doesn't have a more
/// concrete corresponding type.
#[derive(Debug, Default)]
struct FastlyExts {
    fastly_metadata: Option<FastlyResponseMetadata>,
    framing_headers_mode: FramingHeadersMode,
    http_keepalive_mode: HttpKeepaliveMode,
}

impl Into<http::Response<Body>> for Response {
    fn into(self) -> http::Response<Body> {
        let mut resp = http::Response::new(self.body.unwrap_or_else(|| Body::new()));
        resp.extensions_mut().insert(FastlyExts {
            fastly_metadata: self.fastly_metadata,
            framing_headers_mode: self.framing_headers_mode,
            http_keepalive_mode: self.http_keepalive_mode,
        });
        *resp.headers_mut() = self.headers;
        *resp.status_mut() = self.status;
        *resp.version_mut() = self.version;
        resp
    }
}

impl From<http::Response<Body>> for Response {
    fn from(from: http::Response<Body>) -> Self {
        let (mut parts, body) = from.into_parts();
        let fastly_exts: FastlyExts = parts.extensions.remove().unwrap_or_default();
        Response {
            version: parts.version,
            status: parts.status,
            headers: parts.headers,
            body: Some(body),
            fastly_metadata: fastly_exts.fastly_metadata,
            framing_headers_mode: fastly_exts.framing_headers_mode,
            http_keepalive_mode: fastly_exts.http_keepalive_mode,
        }
    }
}

/// Additional Fastly-specific metadata for responses.
#[derive(Debug)]
pub(crate) struct FastlyResponseMetadata {
    backend: Backend,
    sent_req: Option<Request>,
}

impl Clone for FastlyResponseMetadata {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            // sent_req never has a body, so it is fine to clone without it
            sent_req: self.sent_req.as_ref().map(Request::clone_without_body),
        }
    }
}

impl FastlyResponseMetadata {
    /// Create a response metadata object, given the request and the backend name.
    pub fn new(backend: Backend, sent_req: Request) -> Self {
        Self {
            backend,
            sent_req: Some(sent_req),
        }
    }

    /// Get a reference to the backend that this response came from.
    pub fn backend(&self) -> Option<&Backend> {
        // ACF 2020-06-17: this is wrapped in an option for future compatibility when we might have
        // `FastlyResponseMetadata`s on responses that didn't come from a backend
        Some(&self.backend)
    }

    /// Get a reference to the original request associated with this response.
    ///
    /// Note that the request's original body has already been sent, so the returned request does
    /// not have a body.
    pub fn sent_req(&self) -> Option<&Request> {
        self.sent_req.as_ref()
    }

    pub(crate) fn take_sent_req(&mut self) -> Option<Request> {
        self.sent_req.take()
    }
}

/// Send a response to the client with the given HTTP status code, and then panic.
///
/// By default, Rust panics will cause a generic `500 Internal Server Error` response to be sent to
/// the client, if a response has not already been sent. This macro allows you to customize the
/// status code, although the response is still generic.
///
/// The syntax is similar to [`panic!()`], but takes an optional first argument that must implement
/// [`ToStatusCode`], such as [`StatusCode`] or [`u16`]. The optional message and format arguments
/// are passed to [`panic!()`] unchanged, and so will be printed to the logging endpoint specified
/// by [`set_panic_endpoint()`][`crate::log::set_panic_endpoint()`].
///
/// # Examples
///
/// ```no_run
/// # use fastly::{Request, panic_with_status};
/// let req = Request::get("https://example.com/bad_path");
/// if req.get_path().starts_with("bad") {
///     panic_with_status!(403, "forbade request to a bad path: {}", req.get_url_str());
/// }
/// ```
#[macro_export]
macro_rules! panic_with_status {
    () => {
        $crate::panic_with_status!($crate::http::StatusCode::INTERNAL_SERVER_ERROR)
    };
    ($status:expr) => {{
        $crate::Response::new().with_status($status).send_to_client_impl(false, false);
        panic!();
    }};
    ($status:expr, $($arg:tt)*) => {{
        $crate::Response::new().with_status($status).send_to_client_impl(false, false);
        panic!($($arg)*);
    }};
}

/// Make sure a single response is sent to the downstream request
#[doc(hidden)]
pub(crate) fn assert_single_downstream_response_is_sent(panic_on_multiple_send: bool) {
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A flag representing whether or not we have sent a response to the client.
    static SENT: AtomicBool = AtomicBool::new(false);

    // Set our sent flag, and panic if we have already sent a response.
    if SENT.swap(true, Ordering::SeqCst) && panic_on_multiple_send {
        panic!("cannot send more than one client response per execution");
    }
}
