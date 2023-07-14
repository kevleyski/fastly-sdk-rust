//! Automatically enforced limits for HTTP components.
//!
//! When reading in the client request and backend responses, these limits are used to bound the
//! size of various components of the requests and responses. When these limits are exceeded, the
//! program will panic, returning a `400 Bad Request Error` to the client.
//!
//! You can modify these limits, though applications are still subject to the overall WebAssembly
//! heap size limit.
//!
//! # Examples
//!
//! **Changing maximum request header size**
//!
//! ```no_run
//! # use fastly::{Error, Request, Response};
//! use fastly::limits::RequestLimits;
//! fn main() -> Result<(), Error> {
//!     RequestLimits::set_max_header_name_bytes(Some(128));
//!     RequestLimits::set_max_header_value_bytes(Some(128));
//!     let request = Request::from_client();
//!
//!     Response::new().send_to_client();
//!     Ok(())
//! }
//! ```
//!
//! **Changing maximum request URL size**
//!
//! ```no_run
//! # use fastly::{Error, Request, Response};
//! use fastly::limits::RequestLimits;
//! fn main() -> Result<(), Error> {
//!     RequestLimits::set_max_url_bytes(Some(64));
//!     let request = Request::from_client();
//!
//!     Response::new().send_to_client();
//!     Ok(())
//! }
//! ```
//!
//! **Changing maximum response header size**
//!
//! ```no_run
//! # use fastly::{Error, Request, Response};
//! use fastly::limits::ResponseLimits;
//! #[fastly::main]
//! fn main(request: Request) -> Result<Response, Error> {
//!     ResponseLimits::set_max_header_name_bytes(Some(128));
//!     ResponseLimits::set_max_header_value_bytes(Some(128));
//!     let response = request.send("example_backend")?;
//!
//!     Ok(response)
//! }
//! ```
use lazy_static::lazy_static;
use std::sync::RwLock;

pub(crate) const INITIAL_HEADER_NAME_BUF_SIZE: usize = 128;
/// The default header name size limit for [`RequestLimits`] and [`ResponseLimits`].
pub const DEFAULT_MAX_HEADER_NAME_BYTES: usize = 8192;

pub(crate) const INITIAL_HEADER_VALUE_BUF_SIZE: usize = 4096;
/// The default header value size limit for [`RequestLimits`] and [`ResponseLimits`].
pub const DEFAULT_MAX_HEADER_VALUE_BYTES: usize = 8192;

pub(crate) const INITIAL_METHOD_BUF_SIZE: usize = 8;
/// The default method size limit for [`RequestLimits`].
pub const DEFAULT_MAX_METHOD_BYTES: usize = 8192;

pub(crate) const INITIAL_URL_BUF_SIZE: usize = 4096;
/// The default URL size limit for [`RequestLimits`].
pub const DEFAULT_MAX_URL_BYTES: usize = 8192;

pub(crate) const INITIAL_GEO_BUF_SIZE: usize = 1024;

pub(crate) const INITIAL_SECRET_PLAINTEXT_BUF_SIZE: usize = 1024;

lazy_static! {
    pub(crate) static ref REQUEST_LIMITS: RwLock<RequestLimits> =
        RwLock::new(RequestLimits::default());
}

/// The limits for components of an HTTP request.
///
/// This is primarily relevant for the client request, and should be set before the client request
/// is read with a method like [`Request::from_client()`][`crate::Request::from_client()`].
///
/// Since the [`fastly::main`][`crate::main`] attribute macro automatically reads the client request
/// before application code has a chance to run, you should not use the macro if you need to
/// customize the limits.
///
/// # Default values
///
/// | Limit             | Default value                      |
/// |-------------------|------------------------------------|
/// | Header name size  | [`DEFAULT_MAX_HEADER_NAME_BYTES`]  |
/// | Header value size | [`DEFAULT_MAX_HEADER_VALUE_BYTES`] |
/// | Method size       | [`DEFAULT_MAX_METHOD_BYTES`]       |
/// | URL size          | [`DEFAULT_MAX_URL_BYTES`]          |
#[derive(Clone, Copy, Debug)]
pub struct RequestLimits {
    pub(crate) max_header_name_bytes: Option<usize>,
    pub(crate) max_header_value_bytes: Option<usize>,
    pub(crate) max_method_bytes: Option<usize>,
    pub(crate) max_url_bytes: Option<usize>,
}

impl RequestLimits {
    const fn default() -> Self {
        RequestLimits {
            max_header_name_bytes: Some(DEFAULT_MAX_HEADER_NAME_BYTES),
            max_header_value_bytes: Some(DEFAULT_MAX_HEADER_VALUE_BYTES),
            max_method_bytes: Some(DEFAULT_MAX_METHOD_BYTES),
            max_url_bytes: Some(DEFAULT_MAX_URL_BYTES),
        }
    }

    /// Set all request limits to their default values.
    pub fn set_all_default() {
        *REQUEST_LIMITS.write().unwrap() = RequestLimits::default();
    }

    /// Disable all request limits.
    ///
    /// Note that the overall WebAssembly heap size limit still applies.
    pub fn set_all_disabled() {
        *REQUEST_LIMITS.write().unwrap() = RequestLimits {
            max_header_name_bytes: None,
            max_header_value_bytes: None,
            max_method_bytes: None,
            max_url_bytes: None,
        };
    }

    /// Get the current request header name size limit.
    pub fn get_max_header_name_bytes() -> Option<usize> {
        REQUEST_LIMITS.read().unwrap().max_header_name_bytes
    }

    /// Set the request header name size limit.
    pub fn set_max_header_name_bytes(max: Option<usize>) {
        REQUEST_LIMITS.write().unwrap().max_header_name_bytes = max;
    }

    /// Get the current request header value size limit.
    pub fn get_max_header_value_bytes() -> Option<usize> {
        REQUEST_LIMITS.read().unwrap().max_header_value_bytes
    }

    /// Set the request header value size limit.
    pub fn set_max_header_value_bytes(max: Option<usize>) {
        REQUEST_LIMITS.write().unwrap().max_header_value_bytes = max;
    }

    /// Get the current request method size limit.
    pub fn get_max_method_bytes() -> Option<usize> {
        REQUEST_LIMITS.read().unwrap().max_method_bytes
    }

    /// Set the request method size limit.
    pub fn set_max_method_bytes(max: Option<usize>) {
        REQUEST_LIMITS.write().unwrap().max_method_bytes = max;
    }

    /// Get the current request URL size limit.
    pub fn get_max_url_bytes() -> Option<usize> {
        REQUEST_LIMITS.read().unwrap().max_url_bytes
    }

    /// Set the request URL size limit.
    pub fn set_max_url_bytes(max: Option<usize>) {
        REQUEST_LIMITS.write().unwrap().max_url_bytes = max;
    }
}

lazy_static! {
    pub(crate) static ref RESPONSE_LIMITS: RwLock<ResponseLimits> =
        RwLock::new(ResponseLimits::default());
}

/// The limits for components of an HTTP request.
///
/// This is primarily relevant for backend responses, and should be set before sending any backend
/// requests.
///
/// # Default values
///
/// | Limit             | Default value                      |
/// |-------------------|------------------------------------|
/// | Header name size  | [`DEFAULT_MAX_HEADER_NAME_BYTES`]  |
/// | Header value size | [`DEFAULT_MAX_HEADER_VALUE_BYTES`] |
#[derive(Clone, Copy, Debug)]
pub struct ResponseLimits {
    pub(crate) max_header_name_bytes: Option<usize>,
    pub(crate) max_header_value_bytes: Option<usize>,
}

impl ResponseLimits {
    const fn default() -> Self {
        ResponseLimits {
            max_header_name_bytes: None,
            max_header_value_bytes: None,
        }
    }

    /// Set all response limits to their default values.
    pub fn set_all_default() {
        *RESPONSE_LIMITS.write().unwrap() = ResponseLimits::default();
    }

    /// Disable all response limits.
    ///
    /// Note that the overall WebAssembly heap size limit still applies.
    pub fn set_all_disabled() {
        *RESPONSE_LIMITS.write().unwrap() = ResponseLimits {
            max_header_name_bytes: None,
            max_header_value_bytes: None,
        };
    }

    /// Get the current response header name size limit.
    pub fn get_max_header_name_bytes() -> Option<usize> {
        RESPONSE_LIMITS.read().unwrap().max_header_name_bytes
    }

    /// Set the response header name size limit.
    pub fn set_max_header_name_bytes(max: Option<usize>) {
        RESPONSE_LIMITS.write().unwrap().max_header_name_bytes = max;
    }

    /// Get the current response header value size limit.
    pub fn get_max_header_value_bytes() -> Option<usize> {
        RESPONSE_LIMITS.read().unwrap().max_header_value_bytes
    }

    /// Set the response header value size limit.
    pub fn set_max_header_value_bytes(max: Option<usize>) {
        RESPONSE_LIMITS.write().unwrap().max_header_value_bytes = max;
    }
}
