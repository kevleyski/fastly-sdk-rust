//! Compute@Edge Cache APIs.
//!
//! Compute@Edge exposes multiple interfaces to the platform's cache:
//!
//! ## Read-through HTTP caching
//!
//! [`Request::send()`][crate::Request::send()] offers read-through caching for HTTP requests. The
//! HTTP response received from the backend will be cached and reused for subsequent requests if it
//! meets cacheability requirements. The behavior of this automatic caching can be tuned via methods
//! like [`Request::set_ttl()`][crate::Request::set_ttl()] and
//! [`Request::set_pass()`][crate::Request::set_pass()].
//!
//! This interface provides the full benefits of Fastly's purging, request collapsing, and
//! revalidation capabilities, and is recommended for most users who need to cache HTTP responses.
//!
//! ## Simple Cache API
//!
//! The [`simple`] module contains a non-durable key-value API backed by the same cache platform as
//! the [Core Cache API][core], intended to be more accessible for use cases that do not require the
//! full flexibility of that API.
//!
//! ## Core Cache API
//!
//! The [`core`] module exposes the Compute@Edge Core Cache API, the same set of primitive
//! operations used to build Fastly services. The Core Cache API puts the highest level of power in
//! the hands of the user, but requires manual serialization of cache contents and explicit handling
//! of request collapsing and revalidation control flow.

pub mod core;
pub mod simple;
