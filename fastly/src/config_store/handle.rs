//! Low-level Compute@Edge Config Store interfaces.

use crate::abi;
use bytes::BytesMut;
use fastly_shared::FastlyStatus;

/// A low-level interface to Config Store.
///
/// Methods here are typically exposed as the `try_*` APIs on
/// [`crate::config_store::ConfigStore`] and should not be needed directly.
/// Additionally, the [`get()`][`Self::get()`] method allows the caller to configure the
/// size of the buffer used to receive lookup results from the host. The size of this buffer
/// is typically managed by APIs exposed in [`crate::config_store::ConfigStore`].
#[derive(Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct ConfigStoreHandle {
    handle: u32,
}

impl ConfigStoreHandle {
    /// An invalid handle.
    pub const INVALID: Self = ConfigStoreHandle {
        handle: fastly_shared::INVALID_DICTIONARY_HANDLE,
    };

    /// Acquire a handle to an Config Store.
    ///
    /// If a handle could not be acquired, an [`OpenError`] will be returned.
    pub fn open(name: &str) -> Result<Self, OpenError> {
        use OpenError::*;
        let mut handle = Self::INVALID;
        unsafe { abi::fastly_dictionary::open(name.as_ptr(), name.len(), handle.as_u32_mut()) }
            .result()
            .map(|_| handle)
            .map_err(|status| match status {
                FastlyStatus::NONE => NameEmpty,
                FastlyStatus::UNSUPPORTED => NameTooLong,
                FastlyStatus::INVAL => NameInvalid,
                FastlyStatus::BADF => ConfigStoreDoesNotExist,
                _ => panic!("fastly_dictionary::open returned an unrecognized result"),
            })
    }

    /// Lookup a value in this config store.
    ///
    /// If successful, this function returns `Ok(Some(_))` if an entry was found, or `Ok(None)` if
    /// no entry with the given key was found. If the lookup failed, a [`LookupError`] will be
    /// returned.
    pub fn get(&self, key: &str, max_len: usize) -> Result<Option<String>, LookupError> {
        if self.is_invalid() {
            panic!("cannot lookup value with invalid config store handle");
        }
        let mut buf = BytesMut::with_capacity(max_len);
        let mut nwritten = 0;
        let status = unsafe {
            abi::fastly_dictionary::get(
                self.as_u32(),
                key.as_ptr(),
                key.len(),
                buf.as_mut_ptr(),
                buf.capacity(),
                &mut nwritten,
            )
        };
        match status.result().map(|_| nwritten) {
            Ok(nwritten) => {
                assert!(
                    nwritten <= buf.capacity(),
                    "fastly_dictionary::get wrote too many bytes"
                );
                unsafe {
                    buf.set_len(nwritten);
                }
                Ok(Some(
                    String::from_utf8(buf.to_vec()).expect("host returns valid UTF-8"),
                ))
            }
            Err(FastlyStatus::NONE) => Ok(None),
            Err(FastlyStatus::ERROR) => Err(LookupError::Other),
            Err(FastlyStatus::BADF) => Err(LookupError::ConfigStoreInvalid),
            Err(FastlyStatus::INVAL) => Err(LookupError::KeyInvalid),
            Err(FastlyStatus::UNSUPPORTED) => Err(LookupError::KeyTooLong),
            Err(FastlyStatus::BUFLEN) => Err(LookupError::ValueTooLong),
            Err(FastlyStatus::LIMITEXCEEDED) => Err(LookupError::TooManyLookups),
            Err(_) => panic!("fastly_dictionary::get returned an unrecognized result"),
        }
    }

    /// Return true if the config store contains an entry with the given key.
    pub fn contains(&self, key: &str) -> Result<bool, LookupError> {
        use LookupError::*;
        match self.get(key, 0) {
            Ok(Some(_)) | Err(ValueTooLong) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Return true if the [`crate::config_store::ConfigStore`] handle is valid.
    pub fn is_valid(&self) -> bool {
        !self.is_invalid()
    }

    /// Return true if the [`crate::config_store::ConfigStore`] handle is invalid.
    pub fn is_invalid(&self) -> bool {
        self.handle == Self::INVALID.handle
    }

    /// Get the underlying representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub(crate) fn as_u32(&self) -> u32 {
        self.handle
    }

    /// Get a mutable reference to the underlying `u32` representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub(crate) fn as_u32_mut(&mut self) -> &mut u32 {
        &mut self.handle
    }
}

/// Errors thrown when a config store could not be opened.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OpenError {
    /// No config store with this name was found.
    #[error("config store could not be found")]
    ConfigStoreDoesNotExist,
    /// A config store name was empty.
    #[error("config store names cannot be empty")]
    NameEmpty,
    /// A config store name was too long.
    #[error("config store name too long")]
    NameTooLong,
    /// A config store name was invalid.
    #[error("invalid config store name")]
    NameInvalid,
}

/// Errors thrown when a config store lookup failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LookupError {
    /// The config store handle used for a lookup was invalid.
    #[error("invalid config store")]
    ConfigStoreInvalid,
    /// The config store key provided was invalid.
    #[error("invalid config store key")]
    KeyInvalid,
    /// The config store key provided was too long.
    #[error("config store keys must be shorter than 256 characters")]
    KeyTooLong,
    /// The config store value was too long for the provided buffer length.
    #[error("config store value was longer than the given buffer")]
    ValueTooLong,
    /// Too many lookups have been performed
    #[error("Too many lookups have been performed")]
    TooManyLookups,
    /// A config store lookup failed for some other reason.
    #[error("config store lookup failed")]
    Other,
}
