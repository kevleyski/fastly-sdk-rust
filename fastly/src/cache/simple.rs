//! The Compute@Edge Simple Cache API.
//!
//! This is a non-durable key-value API backed by the same cache platform as the [Core Cache
//! API][core].
//!
//! ## Cache scope and purging
//!
//! Cache entries are scoped to Fastly [points of presence
//! (POPs)](https://developer.fastly.com/learning/concepts/pop/): the value set for a key in one POP
//! will not be visible in any other POP.
//!
//! Purging is also scoped to a POP by default, but can be configured to purge globally with
//! Fastly's [purging feature](https://developer.fastly.com/learning/concepts/purging/).
//!
//! ## Interoperability
//!
//! The Simple Cache API is implemented in terms of the [Core Cache API][core]. Items inserted with
//! the Core Cache API can be read by the Simple Cache API, and vice versa. However, some metadata
//! and advanced features like revalidation may be not be available via the Simple Cache API.

use fastly_shared::FastlyStatus;
use sha2::{Digest, Sha256};

use crate::http::purge::purge_surrogate_key;
use crate::Body;

pub use super::core::CacheKey;
use super::core::{self, Transaction};

use std::fmt::Write as _;
use std::time::Duration;

/// Errors arising from cache operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    /// Operation failed due to a limit.
    #[error("Simple Cache operation failed due to a limit")]
    LimitExceeded,
    /// An underlying Core Cache API operation found an invalid state.
    ///
    /// This should not arise during use of this API. If encountered, please report it as a bug.
    #[error("invalid Simple Cache operation; please report this as a bug")]
    InvalidOperation,
    /// Cache operation is not supported.
    #[error("unsupported Simple Cache operation")]
    Unsupported,
    /// An IO error occurred during an operation.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// An error occurred when purging a value.
    #[error("purging error: {0}")]
    Purge(#[source] crate::Error),
    /// An error occurred while running the closure argument of [`get_or_set()`].
    ///
    /// This uses [`anyhow::Error`] to provide maximum flexibility in how the closure reports errors.
    #[error("get_or_set closure error: {0}")]
    GetOrSet(#[source] anyhow::Error),
    /// An unknown error occurred.
    #[error("unknown Simple Cache operation error; please report this as a bug: {0:?}")]
    Other(FastlyStatus),
}

impl From<core::CacheError> for CacheError {
    fn from(value: core::CacheError) -> Self {
        match value {
            core::CacheError::LimitExceeded => Self::LimitExceeded,
            core::CacheError::InvalidOperation => Self::InvalidOperation,
            core::CacheError::Unsupported => Self::Unsupported,
            core::CacheError::Other(st) => Self::Other(st),
        }
    }
}

/// Get the entry associated with the given cache key, if it exists.
///
/// ```no_run
/// # use fastly::cache::simple::*;
/// if let Some(value) = get("my_key").unwrap() {
///     let cached_string = value.into_string();
///     println!("the cached string was: {cached_string}");
/// }
/// ```
///
#[doc = include_str!("../../docs/snippets/key-argument.md")]
pub fn get(key: impl Into<CacheKey>) -> Result<Option<Body>, CacheError> {
    let Some(found) = core::lookup(key.into()).execute()? else {
        return Ok(None);
    };
    Ok(Some(found.to_stream()?))
}

/// Get the entry associated with the given cache key if it exists, or insert and return the
/// specified entry.
///
/// If the value is costly to compute, consider using [`get_or_set_with()`] instead to avoid
/// computation in the case where the value is already present.
///
/// ```no_run
/// # use fastly::cache::simple::*;
/// # use std::time::Duration;
/// let value = get_or_set("my_key", "hello!", Duration::from_secs(60)).unwrap();
/// let cached_string = value.into_string();
/// println!("the cached string was: {cached_string}");
/// ```
///
#[doc = include_str!("../../docs/snippets/key-body-argument.md")]
pub fn get_or_set(
    key: impl Into<CacheKey>,
    value: impl Into<Body>,
    ttl: Duration,
) -> Result<Body, CacheError> {
    get_or_set_with(key, || {
        Ok(CacheEntry {
            value: value.into(),
            ttl,
        })
    })
    .map(|opt| opt.expect("provided closure is infallible"))
}

/// The return type of the closure provided to [`get_or_set_with()`].
#[derive(Debug)]
pub struct CacheEntry {
    /// The value to cache.
    ///
    #[doc = include_str!("../../docs/snippets/body-argument.md")]
    pub value: Body,
    /// The time-to-live for the cache entry.
    pub ttl: Duration,
}

/// Get the entry associated with the given cache key if it exists, or insert and return an entry
/// specified by running the given closure.
///
/// The closure is only run when no value is present for the key, and no other client is in the
/// process of setting it. It takes no arguments, and returns either `Ok` with a [`CacheEntry`]
/// describing the entry to set, or `Err` with an [`anyhow::Error`]. The error is not interpreted by
/// the API, and is solely provided as a user convenience. You can return an error for any reason,
/// and no value will be cached.
///
#[doc = include_str!("../../docs/snippets/key-argument.md")]
///
/// ## Example successful insertion
///
/// ```no_run
/// # use fastly::cache::simple::*;
/// # use std::time::Duration;
/// let value = get_or_set_with("my_key", || {
///     Ok(CacheEntry {
///         value: "hello!".into(),
///         ttl: Duration::from_secs(60),
///     })
/// })
/// .unwrap()
/// .expect("closure always returns `Ok`, so we have a value");
/// let cached_string = value.into_string();
/// println!("the cached string was: {cached_string}");
/// ```
///
/// ## Example unsuccessful insertion
///
/// ```no_run
/// # use fastly::cache::simple::*;
/// let mut tried_to_set = false;
/// let result = get_or_set_with("my_key", || {
///     tried_to_set = true;
///     anyhow::bail!("I changed my mind!")
/// });
/// if tried_to_set {
///     // if our closure was run, we can observe its error in the result
///     assert!(matches!(result, Err(CacheError::GetOrSet(_))));
/// }
/// ```
pub fn get_or_set_with<F>(
    key: impl Into<CacheKey>,
    make_entry: F,
) -> Result<Option<Body>, CacheError>
where
    F: FnOnce() -> Result<CacheEntry, anyhow::Error>,
{
    let key = key.into();
    let lookup_tx = Transaction::lookup(key.clone()).execute()?;
    if !lookup_tx.must_insert_or_update() {
        if let Some(found) = lookup_tx.found() {
            // the value is already present, so just return it
            return Ok(Some(found.to_stream()?));
        } else {
            // we're not in the insert-or-update case, but there's no found?
            return Err(CacheError::InvalidOperation);
        }
    }
    // run the user-provided closure to produce the entry, tagging it as a user error if something
    // goes wrong
    let CacheEntry { value, ttl } = make_entry().map_err(CacheError::GetOrSet)?;
    // perform a standard insert-and-read-back
    let (mut insert_body, found) = lookup_tx
        .insert(ttl)
        .surrogate_keys([
            surrogate_key_for_cache_key(&key, PurgeScope::Pop).as_str(),
            surrogate_key_for_cache_key(&key, PurgeScope::Global).as_str(),
        ])
        .execute_and_stream_back()?;
    insert_body.append(value.into());
    insert_body.finish()?;
    Ok(Some(found.to_stream()?))
}

/// Insert an entry at the given cache key with the given time-to-live.
///
#[doc = include_str!("../../docs/snippets/key-body-argument.md")]
// TODO ACF 2023-06-27: expose this once the invalidation issue is resolved
#[allow(unused)]
fn set(key: impl Into<CacheKey>, value: impl Into<Body>, ttl: Duration) -> Result<(), CacheError> {
    let key = key.into();
    let mut insert_body = core::insert(key.clone(), ttl)
        .surrogate_keys([
            surrogate_key_for_cache_key(&key, PurgeScope::Pop).as_str(),
            surrogate_key_for_cache_key(&key, PurgeScope::Global).as_str(),
        ])
        .execute()?;
    insert_body.append(value.into());
    Ok(insert_body.finish()?)
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum PurgeScope {
    #[default]
    Pop,
    Global,
}

/// Options for [`purge_with_opts()`].
#[derive(Copy, Clone, Debug, Default)]
pub struct PurgeOptions {
    scope: PurgeScope,
}

impl PurgeOptions {
    /// Purge the key from the current POP (default behavior).
    ///
    /// This is the default option used by [`purge()`], and allows a higher throughput of purging
    /// than purging globally.
    pub fn pop_scope() -> Self {
        Self {
            scope: PurgeScope::Pop,
        }
    }

    /// Purge the key globally.
    ///
    #[doc = include_str!("../../docs/snippets/global-purge.md")]
    pub fn global_scope() -> Self {
        Self {
            scope: PurgeScope::Global,
        }
    }
}

/// Purge the entry associated with the given cache key.
///
/// To configure the behavior of the purge, such as to purge globally rather than within the POP,
/// use [`purge_with_opts()`].
///
/// ## Note
///
/// Purged values may persist in cache for a short time (~150ms or less) after this function
/// returns.
///
#[doc = include_str!("../../docs/snippets/key-argument.md")]
pub fn purge(key: impl Into<CacheKey>) -> Result<(), CacheError> {
    purge_surrogate_key(&surrogate_key_for_cache_key(
        &key.into(),
        PurgeOptions::default().scope,
    ))
    .map_err(CacheError::Purge)
}

/// Purge the entry associated with the given cache key.
///
/// The [`PurgeOptions`] argument determines the scope of the purge operation.
///
/// ## Note
///
/// Purged values may persist in cache for a short time (~150ms or less) after this function
/// returns.
///
#[doc = include_str!("../../docs/snippets/key-argument.md")]
///
/// ## Example POP-scoped purge
///
/// ```no_run
/// # use fastly::cache::simple::*;
/// purge_with_opts("my_key", PurgeOptions::pop_scope()).unwrap();
/// ```
///
/// Note that this is the default behavior, and is therefore equivalent to:
///
/// ```no_run
/// # use fastly::cache::simple::*;
/// purge("my_key").unwrap();
/// ```
///
/// ## Example global-scoped purge
///
/// ```no_run
/// # use fastly::cache::simple::*;
/// purge_with_opts("my_key", PurgeOptions::global_scope()).unwrap();
/// ```
///
#[doc = include_str!("../../docs/snippets/global-purge.md")]
pub fn purge_with_opts(key: impl Into<CacheKey>, opts: PurgeOptions) -> Result<(), CacheError> {
    purge_surrogate_key(&surrogate_key_for_cache_key(&key.into(), opts.scope))
        .map_err(CacheError::Purge)
}

/// Create surrogate keys for the given cache key that is compatible with uses of the Simple Cache
/// API.
///
/// Each cache entry for the Simple Cache API is configured with surrogate keys from this function
/// in order to properly scope purge operations for POP-local and global purges. This function is
/// provided as a convenience for implementors wishing to add such a surrogate key manually via the
/// [Core Cache API][core] for interoperability with [`delete()`].
fn surrogate_key_for_cache_key(key: &CacheKey, scope: PurgeScope) -> String {
    let mut sha = Sha256::new();
    sha.update(key);
    if let PurgeScope::Pop = scope {
        // if FASTLY_POP is empty or unavailable for some reason, this will amount to a global purge
        // for now which is the safer choice
        if let Some(pop) = std::env::var_os("FASTLY_POP") {
            sha.update(pop.to_string_lossy().as_bytes());
        }
    }
    let mut sk_str = String::new();
    for b in sha.finalize() {
        write!(&mut sk_str, "{b:02X}").expect("writing to a String is infallible");
    }
    sk_str
}
