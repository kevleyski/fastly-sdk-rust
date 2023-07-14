// Warnings (other than unused variables) in doctests are promoted to errors.
#![doc(test(attr(deny(warnings))))]
#![doc(test(attr(allow(dead_code))))]
#![doc(test(attr(allow(unused_variables))))]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::private_intra_doc_links)]
#![deny(rustdoc::invalid_codeblock_attributes)]

//! # Rust SDK for Compute@Edge.
//!
//! This Rustdoc page is a reference for how to use individual APIs from this SDK. For a guide-level
//! introduction to using Compute@Edge with this SDK, see [Rust on
//! Compute@Edge](https://developer.fastly.com/learning/compute/rust) at the Fastly Developer Hub.
mod abi;

pub mod backend;
pub mod cache;
pub mod config_store;
pub mod convert;
pub mod dictionary;
pub mod error;
pub mod experimental;
pub mod geo;
pub mod handle;
pub mod http;
pub mod kv_store;
pub mod limits;
pub mod log;
pub mod mime;
pub mod object_store;
pub mod secret_store;

pub use crate::backend::Backend;
#[doc(inline)]
pub use crate::config_store::ConfigStore;
#[doc(inline)]
#[allow(deprecated)]
pub use crate::dictionary::Dictionary;
#[doc(inline)]
pub use crate::error::Error;
#[doc(inline)]
pub use crate::http::{Body, Request, Response};
#[doc(inline)]
pub use crate::kv_store::KVStore;
#[doc(inline)]
pub use crate::object_store::ObjectStore;
#[doc(inline)]
pub use crate::secret_store::SecretStore;

pub use fastly_macros::main;

/// Tell the runtime what ABI version this program is using.
///
// TODO ACF 2020-12-02: figure out what we want to do with this function, probably when we switch
// away from using the `fastly-sys` semver for the ABI versioning. For now, hide its documentation
// to avoid confusion for users, but still allow the `#[fastly::main]` macro to see it.
#[doc(hidden)]
pub fn init() {
    unsafe { abi::fastly_abi::init(abi::FASTLY_ABI_VERSION) };
}
