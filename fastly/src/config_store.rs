//! Config Store for Compute@Edge.

pub(crate) mod handle;

use handle::ConfigStoreHandle;
pub use handle::{LookupError, OpenError};

/// Maximum Edge Config Store value size.
///
/// > Config Store containers, item keys, and their values have specific limits. Config Store
/// > containers are limited to 100_000 items. Config Store item keys are limited to 256 characters and
/// > their values are limited to 8000 characters.
///
/// This constant is used as the default buffer size for config store values when using the
/// high-level config store API.
///
const MAX_LEN: usize = 8000;

/// A Compute@Edge Config Store.
pub struct ConfigStore {
    handle: ConfigStoreHandle,
}

impl ConfigStore {
    /// Open a config store, given its name.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::ConfigStore;
    /// let merriam = ConfigStore::open("merriam webster");
    /// let oed = ConfigStore::open("oxford english config store");
    /// ```
    pub fn open(name: &str) -> Self {
        let handle = match ConfigStoreHandle::open(name) {
            Ok(h) if h.is_valid() => h,
            Ok(_) => panic!("could not open config store `{}`", name),
            Err(e) => panic!("could not open config store `{}`: {}", name, e),
        };

        Self { handle }
    }

    /// Try to open a config store, given its name.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::config_store::*;
    /// let merriam = ConfigStore::try_open("merriam webster").unwrap();
    /// ```
    pub fn try_open(name: &str) -> Result<Self, OpenError> {
        ConfigStoreHandle::open(name).map(|handle| Self { handle })
    }

    /// Lookup a value in this config store.
    ///
    /// If successful, this function returns `Some(_)` if an entry was found, or `None` if no entry
    /// with the given key was found.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::ConfigStore;
    /// # let config_store = ConfigStore::open("test config store");
    /// #
    /// assert_eq!(
    ///      config_store.get("bread"),
    ///      Some(String::from("a usually baked and leavened food")),
    /// );
    /// assert_eq!(
    ///     config_store.get("freedom"),
    ///     Some(String::from("the absence of necessity, coercion, or constraint")),
    /// );
    ///
    /// // Otherwise, `get` will return nothing.
    /// assert!(config_store.get("zzzzz").is_none());
    /// ```
    ///
    /// # Panics
    ///
    /// This may panic for any of the reasons that [`ConfigStore::try_get`] would return an error.
    pub fn get(&self, key: &str) -> Option<String> {
        self.try_get(key)
            .unwrap_or_else(|e| panic!("lookup for key `{}` failed: {}", key, e))
    }

    /// Try to lookup a value in this Config Store.
    ///
    /// If successful, this function returns `Ok(Some(_))` if an entry was found, or `Ok(None)` if
    /// no entry with the given key was found. This function returns `Err(_)` if the lookup failed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::ConfigStore;
    /// # let config_store = ConfigStore::open("test config store");
    /// #
    /// assert_eq!(
    ///      config_store.try_get("bread").unwrap(),
    ///      Some(String::from("a usually baked and leavened food")),
    /// );
    /// assert_eq!(
    ///     config_store.try_get("freedom").unwrap(),
    ///     Some(String::from("the absence of necessity, coercion, or constraint")),
    /// );
    ///
    /// // Otherwise, `try_get` will return nothing.
    /// assert!(config_store.try_get("zzzzz").unwrap().is_none());
    /// ```
    pub fn try_get(&self, key: &str) -> Result<Option<String>, LookupError> {
        self.handle.get(key, MAX_LEN)
    }

    /// Return true if the config_store contains an entry with the given key.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use fastly::ConfigStore;
    /// # let config_store = ConfigStore::open("test config_store");
    /// #
    /// assert!(config_store.contains("key"));
    /// ```
    ///
    /// # Panics
    ///
    /// This may panic for any of the reasons that [`ConfigStore::try_get`] would return an error.
    pub fn contains(&self, key: &str) -> bool {
        self.handle
            .contains(key)
            .unwrap_or_else(|e| panic!("lookup for key `{}` failed: {}", key, e))
    }
}
