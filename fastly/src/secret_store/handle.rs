//! Low-level Compute@Edge Secret Store interfaces.

use crate::abi;
use bytes::{Bytes, BytesMut};
use fastly_shared::FastlyStatus;

/// A low-level interface to Secret Store.
///
/// Methods here are typically exposed as the `try_*` APIs on
/// [`crate::secret_store::SecretStore`] and should not be needed directly.
#[derive(Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct SecretStoreHandle {
    handle: u32,
}

impl SecretStoreHandle {
    /// An invalid secret store handle.
    pub const INVALID: Self = SecretStoreHandle {
        handle: fastly_shared::INVALID_SECRET_STORE_HANDLE,
    };

    /// Acquire a handle to a Secret Store.
    ///
    /// If a handle could not be acquired, an [`OpenError`] will be returned.
    pub fn open(secret_store_name: &str) -> Result<Self, OpenError> {
        use OpenError::*;
        let mut handle = Self::INVALID;
        unsafe {
            abi::fastly_secret_store::open(
                secret_store_name.as_ptr(),
                secret_store_name.len(),
                handle.as_u32_mut(),
            )
        }
        .result()
        .map(|_| handle)
        .map_err(|s| match s {
            // If we receive a `None` code, there was no store with this name.
            FastlyStatus::NONE => SecretStoreDoesNotExist(secret_store_name.to_string()),
            // If we receive an `INVAL` code, the given name was not a valid store name.
            FastlyStatus::INVAL => InvalidSecretStoreName(secret_store_name.to_string()),
            _ => Unexpected(s),
        })
    }

    /// Lookup a secret in this Secret Store, and return a handle to it.
    ///
    /// If successful, this function returns `Ok(Some(_))` if a secret was found, or `Ok(None)` if
    /// no secret with the given name was found. If the lookup failed, a [`LookupError`] will be
    /// returned.
    pub fn get(&self, secret_name: &str) -> Result<Option<SecretHandle>, LookupError> {
        use LookupError::*;

        let mut handle = fastly_shared::INVALID_SECRET_HANDLE;
        let status = unsafe {
            abi::fastly_secret_store::get(
                self.as_u32(),
                secret_name.as_ptr(),
                secret_name.len(),
                &mut handle,
            )
        };

        match status {
            FastlyStatus::OK => Ok(Some(SecretHandle { handle })),
            FastlyStatus::NONE => Ok(None),
            FastlyStatus::BADF => Err(InvalidSecretStoreHandle),
            FastlyStatus::INVAL => Err(InvalidSecretName(secret_name.to_string())),
            _ => Err(Unexpected(status)),
        }
    }

    /// Return true if the secret store contains a secret with the given name.
    pub fn contains(&self, name: &str) -> Result<bool, LookupError> {
        match self.get(name) {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
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

/// A low-level interface to a secret.
///
/// Methods here are typically exposed as the APIs on
/// [`crate::secret_store::Secret`] and should not be needed directly.
#[derive(Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct SecretHandle {
    handle: u32,
}

impl SecretHandle {
    /// An invalid secret handle.
    pub const INVALID: Self = SecretHandle {
        handle: fastly_shared::INVALID_SECRET_HANDLE,
    };

    /// Return the plaintext value of this secret.
    //
    // TODO KTM 22-11-01: Assign a distinct `ReadError` error enum to this method, that will let us
    // provide some helpful error messages that don't overlap with looking up secrets from a store.
    pub fn plaintext(&self) -> Result<Bytes, LookupError> {
        use crate::limits::INITIAL_SECRET_PLAINTEXT_BUF_SIZE;

        if self.is_invalid() {
            panic!("cannot lookup plaintext with invalid secret handle");
        }

        // Allocate a mutable byte buffer for our secret's contents.
        let mut plaintext_buf = BytesMut::zeroed(INITIAL_SECRET_PLAINTEXT_BUF_SIZE);
        let mut nwritten = 0usize;

        // Attempt to read the secret's plaintext contents into the buffer.
        let status = unsafe {
            abi::fastly_secret_store::plaintext(
                self.as_u32(),
                plaintext_buf.as_mut_ptr(),
                plaintext_buf.len(),
                &mut nwritten,
            )
        };

        // If the provided buffer was not large enough to fit the plaintext, reserve additional
        // capacity observing the number of bytes needed, in the `nwritten` value. Then, set
        // `nwritten` back to 0 and try again.
        let status = match status {
            FastlyStatus::BUFLEN if nwritten != 0 => {
                plaintext_buf.resize(nwritten, 0);
                nwritten = 0;
                unsafe {
                    abi::fastly_secret_store::plaintext(
                        self.as_u32(),
                        plaintext_buf.as_mut_ptr(),
                        plaintext_buf.len(),
                        &mut nwritten,
                    )
                }
            }
            s => s,
        };

        match status.result() {
            Ok(()) => {
                // Freeze the bytes, being sure to set the length to reflect the number of bytes
                // written into the buffer by the host.
                unsafe {
                    plaintext_buf.set_len(nwritten);
                }
                Ok(plaintext_buf.freeze())
            }
            Err(FastlyStatus::BADF) => Err(LookupError::InvalidSecretHandle),
            Err(FastlyStatus::ERROR) => Err(LookupError::Unexpected(FastlyStatus::ERROR)),
            Err(status) => Err(LookupError::Unexpected(status)),
        }
    }

    /// Create a new "secret" from the given memory. This is not the suggested way to create
    /// [`Secret`]s; instead, we suggest using [`SecretStore::get`].
    ///
    /// This method can be used for data that should be secret, but is being obtained by
    /// some other means than the secret store. New "secrets" created this way use plaintext
    /// only, and live in the session's memory unencrypted for much longer than secrets
    /// generated by [`SecretStore::get`]. They should thus only be used in situations in
    /// which an API requires a [`Secret`], but you cannot (for whatever reason) use a
    /// [`SecretStore`] to store them.
    pub fn new(secret: &[u8]) -> Result<SecretHandle, FastlyStatus> {
        let len = secret.len();

        if len > (64 * 1024) {
            return Err(FastlyStatus::INVAL);
        }

        let ptr = secret.as_ptr();
        let mut handle = fastly_shared::INVALID_SECRET_HANDLE;

        let res = unsafe { fastly_sys::fastly_secret_store::from_bytes(ptr, len, &mut handle) };

        if res != FastlyStatus::OK {
            return Err(res);
        }

        if handle == fastly_shared::INVALID_SECRET_HANDLE {
            return Err(FastlyStatus::ERROR);
        }

        Ok(SecretHandle { handle })
    }

    /// Return true if the [`crate::secret_store::Secret`] handle is invalid.
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
}

/// Errors thrown when a secret store could not be opened.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OpenError {
    /// No secret store exists with the given name.
    #[error("secret store could not be found: {0}")]
    SecretStoreDoesNotExist(String),

    /// The secret store name given is invalid.
    #[error("invalid secret store name: {0}")]
    InvalidSecretStoreName(String),

    /// An unexpected error occurred
    #[error("unexpected error: {0:?}")]
    Unexpected(FastlyStatus),
}

/// Errors thrown when a secret store lookup failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LookupError {
    /// The secret store handle given is invalid.
    #[error("invalid secret store handle")]
    InvalidSecretStoreHandle,

    /// The secret name given is invalid.
    #[error("invalid secret name: {0}")]
    InvalidSecretName(String),

    /// The secret handle given is invalid.
    #[error("invalid secret handle")]
    InvalidSecretHandle,

    /// An unexpected error occurred.
    #[error("unexpected error: {0:?}")]
    Unexpected(FastlyStatus),
}
