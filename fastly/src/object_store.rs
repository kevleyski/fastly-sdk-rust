//! Interface to the Compute@Edge Object Store
#![deprecated(since = "0.9.3", note = "renamed to KV Store")]

#[doc(inline)]
#[deprecated(since = "0.9.3", note = "renamed to KV Store")]
pub use crate::kv_store::KVStore as ObjectStore;

#[doc(inline)]
#[deprecated(since = "0.9.3", note = "renamed to KV Store")]
pub use crate::kv_store::KVStoreError as ObjectStoreError;
