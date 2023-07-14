//! Error-handling utilities.

pub use crate::backend::BackendError;
pub use anyhow::{anyhow, bail, ensure, Context, Error};
use std::fmt;

/// Enum describing what kind of buffer had insufficient size, in a [`BufferSizeError`].
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BufferKind {
    /// The too-small buffer is for holding a [`Geo`][crate::geo::Geo].
    Geo,
    /// The too-small buffer is for holding a header name.
    HeaderName,
    /// The too-small buffer is for holding a header value.
    HeaderValue,
    /// The too-small buffer is for holding an HTTP method.
    HttpMethod,
    /// The too-small buffer is for holding a URL.
    Url,
}

impl fmt::Display for BufferKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BufferKind::Geo => {
                write!(f, "Geo")
            }
            BufferKind::HttpMethod => {
                write!(f, "HTTP method")
            }
            BufferKind::HeaderName => {
                write!(f, "header name")
            }
            BufferKind::HeaderValue => {
                write!(f, "header value")
            }
            BufferKind::Url => {
                write!(f, "URL")
            }
        }
    }
}

/// Insufficient buffer size error.
///
/// This is returned by methods like
/// [`RequestHandle::get_header_names()`][`crate::handle::RequestHandle::get_header_names()`] if a
/// value was larger than the provided maximum size.
///
/// If you get such an error, you can try the same call again with a larger buffer.
#[derive(Copy, Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("insufficient buffer size {buf_size} for buffer '{buffer_kind}'; value requires {needed_buf_size} bytes")]
pub struct BufferSizeError {
    /// The attempted buffer size.
    ///
    /// This is to help make nicer error messages.
    pub buf_size: usize,
    /// The buffer size that was required.
    ///
    /// Trying an operation again with a buffer at least this big may succeed where a previous call
    /// failed. However, it is not guaranteed to succeed, for example if there is an even larger
    /// value later in the list than the first value that was too large.
    pub needed_buf_size: usize,
    /// The buffer kind whose size was insufficient.
    pub buffer_kind: BufferKind,
}

impl BufferSizeError {
    /// Create a new [`BufferSizeError`].
    pub(crate) fn new(buf_size: usize, needed_buf_size: usize, buffer_kind: BufferKind) -> Self {
        Self {
            buf_size,
            needed_buf_size,
            buffer_kind,
        }
    }

    /// Create a new [`BufferSizeError`] for a failure to handle a [`Geo`][crate::geo::Geo].
    pub(crate) fn geo(buf_size: usize, needed_buf_size: usize) -> Self {
        Self::new(buf_size, needed_buf_size, BufferKind::Geo)
    }

    /// Create a new [`BufferSizeError`] for a failure to handle a header name.
    pub(crate) fn header_name(buf_size: usize, needed_buf_size: usize) -> Self {
        Self::new(buf_size, needed_buf_size, BufferKind::HeaderName)
    }

    /// Create a new [`BufferSizeError`] for a failure to handle a header value.
    pub(crate) fn header_value(buf_size: usize, needed_buf_size: usize) -> Self {
        Self::new(buf_size, needed_buf_size, BufferKind::HeaderValue)
    }

    /// Create a new [`BufferSizeError`] for a failure to handle an HTTP method.
    pub(crate) fn http_method(buf_size: usize, needed_buf_size: usize) -> Self {
        Self::new(buf_size, needed_buf_size, BufferKind::HttpMethod)
    }

    /// Create a new [`BufferSizeError`] for a failure to handle a URL.
    pub(crate) fn url(buf_size: usize, needed_buf_size: usize) -> Self {
        Self::new(buf_size, needed_buf_size, BufferKind::Url)
    }
}

#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, thiserror::Error)]
/// `HandleError` is for errors that might arise when using the low level handle
/// interface. For example trying to use a handle for an operation that has
/// already been closed out.
pub enum HandleError {
    #[error("handle for {0} was already closed")]
    /// When using close on a handle, this error will be thrown if the resource
    /// for the handle has been removed from the session already
    ClosedHandle(HandleKind),
    #[error("handle did not exist or was the wrong type")]
    /// A handle did not exist or was the wrong type
    InvalidHandle,
}

#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
/// An enum representing the type of handles that exist within the Compute@Edge
/// platform that point to resources on the host side that users can access or
/// manipulate. Typically this enum is used as part of [`HandleError`] to let
/// you know the type of handle involved when there is an error thrown while
/// using the lower level handle interface
pub enum HandleKind {
    /// This variant corresponds to the [`ResponseHandle`][crate::handle] type
    Response,
    /// This variant corresponds to the [`RequestHandle`][crate::handle] type
    Request,
    /// This variant corresponds to the [`BodyHandle`][crate::handle] type
    Body,
}

impl fmt::Display for HandleKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Response => write!(f, "response"),
            Self::Request => write!(f, "request"),
            Self::Body => write!(f, "body"),
        }
    }
}
