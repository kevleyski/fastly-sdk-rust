//! Dictionaries for Compute@Edge.
#![deprecated(since = "0.8.6", note = "renamed to `config_store`")]
#![allow(deprecated)]
pub(crate) mod handle;

use handle::DictionaryHandle;
pub use handle::{LookupError, OpenError};

/// Maximum Edge Dictionary value size.
///
/// This constant is determined by consulting Fastly's [Edge Dictionary documentation][1]:
///
/// > Dictionary containers, item keys, and their values have specific limits. Dictionary
/// > containers are limited to 1000 items. Dictionary item keys are limited to 256 characters and
/// > their values are limited to 8000 characters.
///
/// This constant is used as the default buffer size for dictionary values when using the
/// high-level dictionary API.
///
/// [1]: https://docs.fastly.com/en/guides/about-edge-dictionaries#limitations-and-considerations
const MAX_LEN: usize = 8000;

/// A Compute@Edge Dictionary.
#[deprecated(since = "0.8.6", note = "renamed to `ConfigStore`")]
pub struct Dictionary {
    handle: DictionaryHandle,
}

impl Dictionary {
    /// Open a dictionary, given its name.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use fastly::Dictionary;
    /// let merriam = Dictionary::open("merriam webster");
    /// let oed = Dictionary::open("oxford english dictionary");
    /// ```
    #[deprecated(since = "0.8.6", note = "use method on `ConfigStore` instead")]
    pub fn open(name: &str) -> Self {
        let handle = match DictionaryHandle::open(name) {
            Ok(h) if h.is_valid() => h,
            Ok(_) => panic!("could not open dictionary `{}`", name),
            Err(e) => panic!("could not open dictionary `{}`: {}", name, e),
        };

        Self { handle }
    }

    /// Try to open a dictionary, given its name.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use fastly::dictionary::*;
    /// let merriam = Dictionary::try_open("merriam webster").unwrap();
    /// ```
    #[deprecated(since = "0.8.6", note = "use method on `ConfigStore` instead")]
    pub fn try_open(name: &str) -> Result<Self, OpenError> {
        DictionaryHandle::open(name).map(|handle| Self { handle })
    }

    /// Lookup a value in this dictionary.
    ///
    /// If successful, this function returns `Some(_)` if an entry was found, or `None` if no entry
    /// with the given key was found.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use fastly::Dictionary;
    /// # let dictionary = Dictionary::open("test dictionary");
    /// #
    /// assert_eq!(
    ///      dictionary.get("bread"),
    ///      Some(String::from("a usually baked and leavened food")),
    /// );
    /// assert_eq!(
    ///     dictionary.get("freedom"),
    ///     Some(String::from("the absence of necessity, coercion, or constraint")),
    /// );
    ///
    /// // Otherwise, `get` will return nothing.
    /// assert!(dictionary.get("zzzzz").is_none());
    /// ```
    ///
    /// # Panics
    ///
    /// This may panic for any of the reasons that [`Dictionary::try_get`] would return an error.
    #[deprecated(since = "0.8.6", note = "use method on `ConfigStore` instead")]
    pub fn get(&self, key: &str) -> Option<String> {
        self.try_get(key)
            .unwrap_or_else(|e| panic!("lookup for key `{}` failed: {}", key, e))
    }

    /// Try to lookup a value in this dictionary.
    ///
    /// If successful, this function returns `Ok(Some(_))` if an entry was found, or `Ok(None)` if
    /// no entry with the given key was found. This function returns `Err(_)` if the lookup failed.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use fastly::Dictionary;
    /// # let dictionary = Dictionary::open("test dictionary");
    /// #
    /// assert_eq!(
    ///      dictionary.try_get("bread").unwrap(),
    ///      Some(String::from("a usually baked and leavened food")),
    /// );
    /// assert_eq!(
    ///     dictionary.try_get("freedom").unwrap(),
    ///     Some(String::from("the absence of necessity, coercion, or constraint")),
    /// );
    ///
    /// // Otherwise, `try_get` will return nothing.
    /// assert!(dictionary.try_get("zzzzz").unwrap().is_none());
    /// ```
    #[deprecated(since = "0.8.6", note = "use method on `ConfigStore` instead")]
    pub fn try_get(&self, key: &str) -> Result<Option<String>, LookupError> {
        self.handle.get(key, MAX_LEN)
    }

    /// Return true if the dictionary contains an entry with the given key.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use fastly::Dictionary;
    /// # let dictionary = Dictionary::open("test dictionary");
    /// #
    /// assert!(dictionary.contains("key"));
    /// ```
    ///
    /// # Panics
    ///
    /// This may panic for any of the reasons that [`Dictionary::try_get`] would return an error.
    #[deprecated(since = "0.8.6", note = "use method on `ConfigStore` instead")]
    pub fn contains(&self, key: &str) -> bool {
        self.handle
            .contains(key)
            .unwrap_or_else(|e| panic!("lookup for key `{}` failed: {}", key, e))
    }
}
