//! [Media types][mdn] (also known as Multipurpose Internet Mail Extensions or MIME types).
//!
//! This module re-exports the [`mime`][`::mime`] crate for convenient use in Compute@Edge
//! programs. See the [`mime`][`::mime`] documentation and [MDN][mdn] for details.
//!
//! [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Basics_of_HTTP/MIME_types
#[doc(inline)]
pub use ::mime::*;
