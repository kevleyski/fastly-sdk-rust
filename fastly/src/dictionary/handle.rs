#![deprecated(since = "0.8.6", note = "renamed to `handle::config_store`")]
//! Low-level Compute@Edge Dictionary interfaces.

use crate::abi;
use bytes::BytesMut;
use fastly_shared::FastlyStatus;

/// A low-level interface to Edge Dictionaries.
///
/// Unlike the high-level [`Dictionary`][`crate::dictionary::Dictionary`], this type has methods
/// that return `Result` values upon failure, rather than panicking. Additionally, the
/// [`get()`][`Self::get()`] method allows the caller to configure the size of the buffer used to
/// received lookup results from the host.
#[derive(Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
#[deprecated(since = "0.8.6", note = "renamed to `ConfigStoreHandle`")]
pub struct DictionaryHandle {
    handle: u32,
}

impl DictionaryHandle {
    /// An invalid handle.
    #[deprecated(
        since = "0.8.6",
        note = "use the constant in `ConfigStoreHandle` instead"
    )]
    pub const INVALID: Self = DictionaryHandle {
        handle: fastly_shared::INVALID_DICTIONARY_HANDLE,
    };

    /// Acquire a handle to an Edge Dictionary.
    ///
    /// If a handle could not be acquired, an [`OpenError`] will be returned.
    #[deprecated(
        since = "0.8.6",
        note = "use the method on `ConfigStoreHandle` instead"
    )]
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
                FastlyStatus::BADF => DictionaryDoesNotExist,
                _ => panic!("fastly_dictionary::open returned an unrecognized result"),
            })
    }

    /// Lookup a value in this dictionary.
    ///
    /// If successful, this function returns `Ok(Some(_))` if an entry was found, or `Ok(None)` if
    /// no entry with the given key was found. If the lookup failed, a [`LookupError`] will be
    /// returned.
    #[deprecated(
        since = "0.8.6",
        note = "use the method on `ConfigStoreHandle` instead"
    )]
    pub fn get(&self, key: &str, max_len: usize) -> Result<Option<String>, LookupError> {
        if self.is_invalid() {
            panic!("cannot lookup value with invalid dictionary handle");
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
            Err(FastlyStatus::BADF) => Err(LookupError::DictionaryInvalid),
            Err(FastlyStatus::INVAL) => Err(LookupError::KeyInvalid),
            Err(FastlyStatus::UNSUPPORTED) => Err(LookupError::KeyTooLong),
            Err(FastlyStatus::BUFLEN) => Err(LookupError::ValueTooLong),
            Err(FastlyStatus::LIMITEXCEEDED) => Err(LookupError::TooManyLookups),
            Err(_) => panic!("fastly_dictionary::get returned an unrecognized result"),
        }
    }

    /// Return true if the dictionary contains an entry with the given key.
    #[deprecated(
        since = "0.8.6",
        note = "use the method on `ConfigStoreHandle` instead"
    )]
    pub fn contains(&self, key: &str) -> Result<bool, LookupError> {
        use LookupError::*;
        match self.get(key, 0) {
            Ok(Some(_)) | Err(ValueTooLong) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Return true if the request handle is valid.
    #[deprecated(
        since = "0.8.6",
        note = "use the method on `ConfigStoreHandle` instead"
    )]
    pub fn is_valid(&self) -> bool {
        !self.is_invalid()
    }

    /// Return true if the request handle is invalid.
    #[deprecated(
        since = "0.8.6",
        note = "use the method on `ConfigStoreHandle` instead"
    )]
    pub fn is_invalid(&self) -> bool {
        self.handle == Self::INVALID.handle
    }

    /// Get the underlying representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    #[deprecated(
        since = "0.8.6",
        note = "use the method on `ConfigStoreHandle` instead"
    )]
    pub(crate) fn as_u32(&self) -> u32 {
        self.handle
    }

    /// Get a mutable reference to the underlying `u32` representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    #[deprecated(
        since = "0.8.6",
        note = "use the method on `ConfigStoreHandle` instead"
    )]
    pub(crate) fn as_u32_mut(&mut self) -> &mut u32 {
        &mut self.handle
    }
}

/// Errors thrown when a dictionary could not be opened.
#[derive(Debug, thiserror::Error)]
#[deprecated(since = "0.8.6", note = "use `config_store::OpenError` instead")]
#[non_exhaustive]
pub enum OpenError {
    /// No dictionary with this name was found.
    #[error("dictionary could not be found")]
    DictionaryDoesNotExist,
    /// A dictionary name was empty.
    #[error("dictionary names cannot be empty")]
    NameEmpty,
    /// A dictionary name was too long.
    #[error("dictionary name too long")]
    NameTooLong,
    /// A dictionary name was invalid.
    #[error("invalid dictionary name")]
    NameInvalid,
}

/// Errors thrown when a dictionary lookup failed.
#[derive(Debug, thiserror::Error)]
#[deprecated(since = "0.8.6", note = "use `config_store::LookupError` instead")]
#[non_exhaustive]
pub enum LookupError {
    /// The dictionary handle used for a lookup was invalid.
    #[error("invalid dictionary")]
    DictionaryInvalid,
    /// The dictionary key provided was invalid.
    #[error("invalid dictionary key")]
    KeyInvalid,
    /// The dictionary key provided was too long.
    #[error("dictionary keys must be shorter than 256 characters")]
    KeyTooLong,
    /// The dictionary value was too long for the provided buffer length.
    #[error("dictionary value was longer than the given buffer")]
    ValueTooLong,
    /// Too many lookups have been performed
    #[error("Too many lookups have been performed")]
    TooManyLookups,
    /// A dictionary lookup failed for some other reason.
    #[error("dictionary lookup failed")]
    Other,
}
