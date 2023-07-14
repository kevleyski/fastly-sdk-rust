//! Low-level interfaces to the Compute@Edge APIs.
//!
//! For most applications, you should instead use the types in the top level of the crate, such as
//! [`Request`][`crate::Request`] and [`Response`][`crate::Response`].
//!
//! # Reasons not to use handles
//!
//! - The high-level interface has many more conveniences for application development. For example,
//!   there are methods for transparently reading and writing HTTP bodies as JSON, and common
//!   function argument types such as header values can accept and convert a variety of types
//!   automatically.
//!
//! - [`BodyHandle`] and [`StreamingBodyHandle`] are unbuffered. Performance can suffer dramatically
//!   if repeated small reads and writes are made to these types. The higher-level equivalents,
//!   [`Body`][`crate::Body`] and [`StreamingBody`][`crate::http::body::StreamingBody`] are buffered
//!   automatically, though you can explicitly control some aspects of the buffering using
//!   [`std::io::BufRead`] and [`std::io::Write::flush()`].
//!
//! - Explicit buffer sizes are required to get data such as header values from the Compute@Edge
//!   host. If the size you choose isn't large enough, the operation will fail with an error and
//!   make you try again. The high-level interfaces automatically retry any such operations with the
//!   necessary buffer sizes, within limits set by the [`limits`][`crate::limits`] module.
//!
//! - The high-level interface keeps data about a request or response in WebAssembly memory until it
//!   is sent to the client or a backend, whereas the handle interface is backed by memory in the
//!   Compute@Edge host.
//!
//!   Suppose your application needs to manipulate headers in multiple functions. The handle
//!   interface would require you to either manually keep track of the headers separately from the
//!   handle they came from, or perform redundant copies to and from WebAssembly memory. The
//!   high-level interface would keep all of your header information in WebAssembly until it's ready
//!   to use, improving performance.
//!
//! # When to use handles
//!
//! The list of cases where we recommend using the handle interface is rather short, but that
//! doesn't mean there aren't more that we haven't thought of. If you find more cases where the
//! handle interface gives you an advantage over the high-level interface, [we would love to hear
//! from you](mailto:oss@fastly.com)!
//!
//! - If your application needs to forward requests or responses with very large headers, but never
//!   needs to inspect or log those headers, the handle interface will allow you to avoid copying
//!   those headers into and out of WebAssembly memory unnecessarily.
//!
//! - If you are building your own higher-level abstractions for HTTP, or connecting the `fastly`
//!   crate to another HTTP library ecosystem, you may find it more direct to use the handle
//!   interface. Do note, however, that the high-level [`Request`][`crate::Request`] and
//!   [`Response`][`crate::Response`] types can be cheaply converted to and from [`http::Request`]
//!   and [`http::Response`], which are widely used by other libraries.
pub use crate::http::body::handle::BodyHandle;
pub use crate::http::body::streaming::handle::StreamingBodyHandle;
pub use crate::http::request::handle::{
    client_h2_fingerprint, client_ip_addr, client_original_header_count,
    client_original_header_names, client_request_and_body, client_request_id,
    client_tls_cipher_openssl_name, client_tls_client_hello, client_tls_ja3_md5,
    client_tls_protocol, RequestHandle,
};
pub use crate::http::request::pending::{select_handles, PendingRequestHandle, PollHandleResult};
pub use crate::http::response::handle::ResponseHandle;
pub use fastly_shared::CacheOverride;

/// Low-level Compute@Edge Dictionary interfaces.
#[deprecated(since = "0.8.6", note = "renamed to `config_store`")]
pub mod dictionary {
    #[allow(deprecated)]
    pub use crate::dictionary::handle::*;
}

/// Low-level Compute@Edge Config Store interfaces.
pub mod config_store {
    pub use crate::config_store::handle::*;
}
