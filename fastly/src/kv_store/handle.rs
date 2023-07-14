//! Safe abstractions around the KV Store FFI.
use fastly_shared::{FastlyStatus, INVALID_BODY_HANDLE, INVALID_KV_STORE_HANDLE};
use fastly_sys::fastly_kv_store as sys;

use crate::handle::BodyHandle;

/// Errors that can arise during KV Store operations.
///
/// This type is marked as non-exhaustive because more variants will be added over time.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum KVStoreError {
    /// The key provided for this operation was not valid.
    #[error("Invalid KV Store key")]
    InvalidKey,
    /// The Object Store handle provided for this operation was not valid.
    #[error("Invalid Object Store handle")]
    #[deprecated(since = "0.9.3", note = "renamed to KV Store")]
    InvalidObjectStoreHandle,
    /// The KV Store handle provided for this operation was not valid.
    #[error("Invalid KV Store handle")]
    InvalidKVStoreHandle,
    /// No Object Store by this name exists.
    #[error("Object Store {0:?} not found")]
    #[deprecated(since = "0.9.3", note = "renamed to KV Store")]
    ObjectStoreNotFound(String),
    /// No KV Store by this name exists.
    #[error("KV Store {0:?} not found")]
    KVStoreNotFound(String),
    /// Some unexpected error occurred.
    #[error("Unexpected KV Store error: {0:?}")]
    Unexpected(FastlyStatus),
}

impl From<FastlyStatus> for KVStoreError {
    fn from(st: FastlyStatus) -> Self {
        KVStoreError::Unexpected(st)
    }
}

/// A handle to a key-value store that is open for lookups and inserting.
#[derive(Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct StoreHandle {
    handle: u32,
}

impl StoreHandle {
    /// Get the underlying representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub fn as_u32(&self) -> u32 {
        self.handle
    }

    /// Open a handle to the KV Store with the given name.
    pub fn open(name: &str) -> Result<Option<StoreHandle>, KVStoreError> {
        let mut store_handle_out = INVALID_KV_STORE_HANDLE;
        let status = unsafe { sys::open(name.as_ptr(), name.len(), &mut store_handle_out) };
        status.result().map_err(|st| match st {
            FastlyStatus::INVAL => KVStoreError::KVStoreNotFound(name.to_owned()),
            _ => st.into(),
        })?;
        if store_handle_out == INVALID_KV_STORE_HANDLE {
            Ok(None)
        } else {
            Ok(Some(StoreHandle {
                handle: store_handle_out,
            }))
        }
    }

    /// Look up a value in the KV Store.
    ///
    /// Returns `Ok(Some(BodyHandle))` if a value is found, and `Ok(None)` if the key was not
    /// found or is expired.
    pub fn lookup(&self, key: impl AsRef<[u8]>) -> Result<Option<BodyHandle>, KVStoreError> {
        let mut body_handle_out = INVALID_BODY_HANDLE;
        let key = key.as_ref();
        let status =
            unsafe { sys::lookup(self.as_u32(), key.as_ptr(), key.len(), &mut body_handle_out) };
        status.result().map_err(|st| match st {
            FastlyStatus::BADF => KVStoreError::InvalidKVStoreHandle,
            FastlyStatus::INVAL => KVStoreError::InvalidKey,
            _ => st.into(),
        })?;
        if body_handle_out == INVALID_BODY_HANDLE {
            Ok(None)
        } else {
            Ok(Some(unsafe { BodyHandle::from_u32(body_handle_out) }))
        }
    }

    /// Insert a value into the KV Store.
    ///
    /// If the KV Store already contains a value for this key, it will be overwritten.
    pub fn insert(&mut self, key: impl AsRef<str>, value: BodyHandle) -> Result<(), KVStoreError> {
        let key = key.as_ref();
        let status =
            unsafe { sys::insert(self.as_u32(), key.as_ptr(), key.len(), value.into_u32()) };
        status.result().map_err(|st| match st {
            FastlyStatus::BADF => KVStoreError::InvalidKVStoreHandle,
            FastlyStatus::INVAL => KVStoreError::InvalidKey,
            _ => st.into(),
        })?;
        Ok(())
    }
}
