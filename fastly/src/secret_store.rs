//! Interface to the Compute@Edge Secret Store.

pub use self::handle::{LookupError, OpenError};

use self::handle::{SecretHandle, SecretStoreHandle};
use bytes::Bytes;

pub(crate) mod handle;

/// A Compute@Edge Secret Store.
///
/// A secret store name has a maximum length of 255 bytes and must
/// contain only letters, numbers, dashes (-), underscores (_), and
/// periods (.).
pub struct SecretStore {
    handle: SecretStoreHandle,
}

impl SecretStore {
    /// Open the Secret Store with the given name.
    pub fn open(name: &str) -> Result<Self, OpenError> {
        SecretStoreHandle::open(name).map(|handle| Self { handle })
    }

    /// Lookup a [`Secret`] by name in this secret store.
    ///
    /// Returns `Some(secret)` if the secret is found, and `None` if the secret was not found.
    ///
    /// See [`try_get()`][`SecretStore::try_get()`] for a fallible equivalent of this method.
    pub fn get(&self, name: &str) -> Option<Secret> {
        self.try_get(name)
            .unwrap_or_else(|e| panic!("lookup for secret `{}` failed: {}", name, e))
    }

    /// Try to lookup a [`Secret`] by name in this secret store.
    ///
    /// If successful, this method returns `Ok(Some(secret))` if the secret is found, or `Ok(None)`
    /// if the secret was not found.
    pub fn try_get(&self, name: &str) -> Result<Option<Secret>, LookupError> {
        let handle = match self.handle.get(name)? {
            Some(h) => h,
            None => return Ok(None),
        };
        let secret = Secret {
            name: name.to_owned(),
            handle,
            plaintext: std::cell::RefCell::new(None),
        };
        Ok(Some(secret))
    }

    /// Return true if the secret store contains a secret with the given
    /// name.
    pub fn contains(&self, name: &str) -> Result<bool, LookupError> {
        self.handle.contains(name)
    }
}

/// A secret from a secret store.
///
/// A secret name has a maximum length of 255 bytes and must contain
/// only letters, numbers, dashes (-), underscores (_), and periods (.).
///
/// A secret value has a maximum length of 64 KiB.
pub struct Secret {
    name: String,
    handle: SecretHandle,
    plaintext: std::cell::RefCell<Option<Bytes>>,
}

impl Secret {
    /// Read the plaintext contents of a secret into memory as a byte buffer.
    ///
    /// Once a secret is read into memory, a secret's contents can be repeatedly accessed cheaply.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::secret_store::SecretStore;
    /// # let secret_store = SecretStore::open("secret store").unwrap();
    /// let secret = secret_store.get("example").unwrap();
    /// assert_eq!(secret.plaintext(), "hello world!")
    /// ```
    ///
    /// Check if a [`HeaderValue`][`http::HeaderValue`] matches the contents of a secret.
    ///
    /// ```no_run
    /// # use fastly::Request;
    /// # use fastly::secret_store::SecretStore;
    /// # let secret_store = SecretStore::open("secret store").unwrap();
    /// # let request = Request::from_client();
    /// let secret = secret_store.get("example").unwrap();
    /// let header = request.get_header("example").unwrap();
    /// if secret.plaintext() == header.as_bytes() {
    ///     println!("you have guessed correctly!");
    /// }
    /// ```
    pub fn plaintext(&self) -> Bytes {
        use std::ops::Deref;

        // If we have already paged the plaintext contents of this secret into memory, we can
        // cheaply clone a new reference to the existing byte buffer. We are done!
        if let Some(plaintext) = self.plaintext.borrow().deref() {
            return plaintext.clone();
        }

        // Use our secret handle to read the contents of the secret into a byte buffer.
        let bytes = self
            .handle
            .plaintext()
            .unwrap_or_else(|e| panic!("lookup for secret `{}` failed: {}", self.name, e));

        // Before we return store a reference to the bytes, so that future reads are amortized.
        self.plaintext.borrow_mut().replace(bytes.clone());

        bytes
    }

    /// Create a new "secret" from the given memory. This is *not* the suggested way to create
    /// [`Secret`]s; instead, we suggest using [`SecretStore::get`]. This secret will *NOT* be
    /// shared with other sessions.
    ///
    /// This method can be used for data that should be secret, but is being obtained by
    /// some other means than the secret store. New "secrets" created this way use plaintext
    /// only, and live in the session's memory unencrypted for much longer than secrets
    /// generated by [`SecretStore::get`]. They should thus only be used in situations in
    /// which an API requires a [`Secret`], but you cannot (for whatever reason) use a
    /// [`SecretStore`] to store them.
    ///
    /// As the early note says, this [`Secret`] will be local to the current session, and
    /// will not be shared with other sessions of this service.
    // NOTE: I've chosen not to make this an instance of `From` specifically to ensure that
    // people using this API get a chance to read the caveats in the above message.
    pub fn from_bytes(secret: Vec<u8>) -> Result<Self, fastly_shared::FastlyStatus> {
        let handle = SecretHandle::new(&secret)?;

        Ok(Secret {
            name: "<generated>".to_string(),
            handle: handle,
            plaintext: std::cell::RefCell::new(Some(secret.into())),
        })
    }
}
