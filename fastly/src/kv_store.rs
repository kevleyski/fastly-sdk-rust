//! Interface to the [Compute@Edge KV Store][blog].
//!
//! [blog]: https://www.fastly.com/blog/introducing-the-compute-edge-kv-store-global-persistent-storage-for-compute-functions

use crate::Body;

pub use self::handle::KVStoreError;
use self::handle::StoreHandle;

// TODO ACF 2022-10-10: this module is temporarily public for the large kv preview.
#[doc(hidden)]
pub mod handle;

/// A [Compute@Edge KV Store][blog].
///
/// Keys in the KV Store must follow the following rules:
///
///   * Keys can contain any sequence of valid Unicode characters, of length 1-1024 bytes when
///     UTF-8 encoded.
///   * Keys cannot contain Carriage Return or Line Feed characters.
///   * Keys cannot start with `.well-known/acme-challenge/`.
///   * Keys cannot be named `.` or `..`.
///
/// [blog]: https://www.fastly.com/blog/introducing-the-compute-edge-kv-store-global-persistent-storage-for-compute-functions
pub struct KVStore {
    handle: StoreHandle,
}

impl KVStore {
    // TODO ACF 2022-10-10: temporary method to support the large kv preview
    #[doc(hidden)]
    pub fn as_handle(&self) -> &StoreHandle {
        &self.handle
    }

    /// Open the KV Store with the given name.
    ///
    /// If there is no store by that name, this returns `Ok(None)`.
    pub fn open(name: &str) -> Result<Option<Self>, KVStoreError> {
        match StoreHandle::open(name)? {
            Some(handle) => Ok(Some(Self { handle })),
            None => Ok(None),
        }
    }

    /// Look up a value in the KV Store.
    ///
    /// Returns `Ok(Some(Body))` if the value is found, and `Ok(None)` if the key was not found.
    pub fn lookup(&self, key: &str) -> Result<Option<Body>, KVStoreError> {
        Ok(self
            .handle
            .lookup(key.as_bytes())?
            .map(|body_handle| body_handle.into()))
    }

    /// Look up a value in the KV Store, and return it as a byte vector.
    ///
    /// Returns `Ok(Some(Vec<u8>))` if the value is found, and `Ok(None)` if the key was not found.
    pub fn lookup_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, KVStoreError> {
        Ok(self.lookup(key)?.map(|body| body.into_bytes()))
    }

    /// Look up a value in the KV Store, and return it as a string.
    ///
    /// Returns `Ok(Some(String))` if the value is found, and `Ok(None)` if the key was not found.
    ///
    /// # Panics
    ///
    /// Panics if the value is not a valid UTF-8 string.
    pub fn lookup_str(&self, key: &str) -> Result<Option<String>, KVStoreError> {
        Ok(self.lookup(key)?.map(|body| body.into_string()))
    }

    /// Insert a value into the KV Store.
    ///
    /// If the store already contained a value for this key, it will be overwritten.
    ///
    /// The value may be provided as any type that can be converted to [`Body`], such as `&[u8]`,
    /// `Vec<u8>`, `&str`, or `String`.
    ///
    /// # Value sizes
    ///
    /// The size of the value must be known when calling this method. In practice, that means that
    /// if a [`Body`] value contains an external request or response, it must be encoded with
    /// `Content-Length` rather than `Transfer-Encoding: chunked`.
    ///
    /// For the moment, this method will return `StoreError::Unexpected(FastlyStatus::INVAL)` if the
    /// value size is not known. This will be replaced by a more specific error value in a future
    /// release.
    pub fn insert(&mut self, key: &str, value: impl Into<Body>) -> Result<(), KVStoreError> {
        self.handle.insert(key, value.into().into_handle())
    }
}
