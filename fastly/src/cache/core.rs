//! The Compute@Edge Core Cache API.
//!
//! This API exposes the primitive operations required to implement high-performance cache
//! applications with advanced features such as [request
//! collapsing](https://developer.fastly.com/learning/concepts/request-collapsing/), [streaming
//! miss](https://docs.fastly.com/en/guides/streaming-miss),
//! [revalidation](https://developer.fastly.com/learning/concepts/stale/), and [surrogate key
//! purging](https://docs.fastly.com/en/guides/purging-api-cache-with-surrogate-keys).
//!
//! While this API contains affordances for some HTTP caching concepts such as `Vary` headers and
//! `stale-while-revalidate`, this API is **not** suitable for HTTP caching out-of-the-box. Future
//! SDK releases will add a more customizable HTTP Cache API with support for customizable
//! read-through caching, freshness lifetime inference, conditional request evaluation, automatic
//! revalidation, and more.
//!
//! Cached items in this API consist of:
//!
//! * **A cache key**: up to 4KiB of arbitrary bytes that identify a cached item. The cache key may
//!   not uniquely identify an item; **headers** can be used to augment the key when multiple items
//!   are associated with the same key. See [`LookupBuilder::header()`] for more details.
//!
//! * **General metadata**, such as expiry data (item age, when to expire, and surrogate keys for
//!   purging).
//!
//! * **User-controlled metadata**: arbitrary bytes stored alongside the cached item contents that
//!   can be updated when revalidating the cached item.
//!
//! * **The object itself**: arbitrary bytes read via [`Body`] and written via [`StreamingBody`].
//!
//! In the simplest cases, the top-level [`insert()`] and [`lookup()`] functions are used for
//! one-off operations on a cached item, and are appropriate when request collapsing and
//! revalidation capabilities are not required.
//!
//! The API also supports more complex uses via [`Transaction`], which can collapse concurrent
//! lookups to the same item, including coordinating revalidation. See the [`Transaction`]
//! documentation for more details.

use self::handle::{
    CacheHandle, GetBodyOptions, LookupOptions as HandleLookupOptions,
    WriteOptions as HandleWriteOptions,
};
use crate::{
    convert::{ToHeaderName, ToHeaderValue},
    handle::RequestHandle,
    http::{
        body::{Body, StreamingBody},
        HeaderName, HeaderValue,
    },
};
use bytes::Bytes;
use fastly_shared::FastlyStatus;
use std::{sync::Arc, time::Duration};

mod handle;
pub use handle::CacheKey;
use handle::CacheLookupState;

/// Errors arising from cache operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    /// Operation failed due to a limit.
    #[error("cache operation failed due to a limit")]
    LimitExceeded,
    /// Operation was not valid to be performed given the state of the cached item.
    #[error("invalid cache operation")]
    InvalidOperation,
    /// Cache operation is not supported.
    #[error("unsupported cache operation")]
    Unsupported,
    /// An unknown error occurred.
    #[error("unknown cache operation error; please report this as a bug: {0:?}")]
    Other(FastlyStatus),
}

impl From<FastlyStatus> for CacheError {
    fn from(status: FastlyStatus) -> Self {
        match status {
            FastlyStatus::UNSUPPORTED => CacheError::Unsupported,
            FastlyStatus::LIMITEXCEEDED => CacheError::LimitExceeded,
            // This case is specifically for streaming the body, which is the only place it's expected
            FastlyStatus::BADF => CacheError::InvalidOperation,
            other => CacheError::Other(other),
        }
    }
}

/// An owned variant of `HandleLookupOptions`.
#[derive(Default)]
struct LookupOptions {
    request_headers: Option<RequestHandle>,
}

impl LookupOptions {
    fn as_handle_options(&self) -> HandleLookupOptions {
        HandleLookupOptions {
            request_headers: self.request_headers.as_ref(),
        }
    }
}

/// A builder-style API for configuring a non-transactional lookup.
pub struct LookupBuilder {
    key: CacheKey,
    options: LookupOptions,
}

/// Returns a [`LookupBuilder`] that will perform a non-transactional cache lookup.
///
/// ```no_run
/// # use fastly::cache::core::*;
/// # use std::io::Read;
/// let mut cached_string = String::new();
/// if let Some(entry) = lookup(CacheKey::from_static(b"my_key")).execute().unwrap() {
///     entry
///         .to_stream()
///         .unwrap()
///         .read_to_string(&mut cached_string)
///         .unwrap();
/// }
/// println!("the cached string was: {cached_string}");
/// ```
///
/// # Relationship with [`Transaction::lookup()`]
///
/// In contrast to [`Transaction::lookup()`], a non-transactional `lookup` will not attempt to
/// coordinate with any concurrent cache lookups. If two instances of the service perform a `lookup`
/// at the same time for the same cache key, and the item is not yet cached, they will both get
/// `Ok(None)` from the eventual lookup execution. Without further coordination, they may both end
/// up performing the work needed to [`insert()`] the item (which usually involves origin requests
/// and/or computation) and racing with each other to insert.
///
/// To resolve such races between concurrent lookups, use [`Transaction::lookup()`] instead.
pub fn lookup(key: CacheKey) -> LookupBuilder {
    LookupBuilder {
        key,
        options: LookupOptions::default(),
    }
}

impl LookupBuilder {
    /// Sets a multi-value header for this lookup, discarding any previous values associated
    /// with the header `name`.
    ///
    #[doc = include_str!("../../docs/snippets/cache-headers.md")]
    pub fn header_values<'a>(
        mut self,
        name: impl ToHeaderName,
        values: impl IntoIterator<Item = &'a HeaderValue>,
    ) -> Self {
        self.options
            .request_headers
            .get_or_insert_with(RequestHandle::new)
            .set_header_values(&name.into_owned(), values);
        self
    }

    /// Sets a single-value header for this lookup, discarding any previous values associated
    /// with the header `name`.
    ///
    #[doc = include_str!("../../docs/snippets/cache-headers.md")]
    pub fn header(self, name: impl ToHeaderName, value: impl ToHeaderValue) -> Self {
        self.header_values(&name.into_owned(), Some(&value.into_owned()))
    }

    /// Perform the lookup, returning a [`Found`] object if a usable cached item was found.
    ///
    /// A cached item is _usable_ if its age is less than the sum of its TTL and its
    /// stale-while-revalidate period. Items beyond that age are unusably stale.
    pub fn execute(self) -> Result<Option<Found>, CacheError> {
        let cache_handle = handle::lookup(self.key, &self.options.as_handle_options())?;
        // force the lookup to await in the host, as we want synchronous behavior here.
        cache_handle.wait()?;
        if cache_handle.get_state().contains(CacheLookupState::FOUND) {
            Ok(Some(Found {
                handle: Arc::new(cache_handle),
            }))
        } else {
            Ok(None)
        }
    }
}

/// A cached item returned by a lookup.
///
/// This type can be used to [get the cached item as a stream][Found::to_stream()], and to retrieve
/// its metadata, such as [its size][Found::known_length()] or [whether it's
/// stale][Found::is_stale()].
pub struct Found {
    // The `Arc` allows for a cache handle to be shared with `Transaction` in the transactional case.
    handle: Arc<CacheHandle>,
}

impl Found {
    /// The time for which the cached item is considered fresh.
    pub fn ttl(&self) -> Duration {
        Duration::from_nanos(
            self.handle
                .get_max_age_ns()
                .expect("`Found` is missing max age metadata"),
        )
    }

    /// The current age of the cached item.
    pub fn age(&self) -> Duration {
        Duration::from_nanos(
            self.handle
                .get_age_ns()
                .expect("`Found` is missing age metadata"),
        )
    }

    /// The time for which a cached item can safely be used despite being considered stale.
    pub fn stale_while_revalidate(&self) -> Duration {
        Duration::from_nanos(
            self.handle
                .get_stale_while_revalidate_ns()
                .expect("`Found` is missing stale_while_revalidate metadata"),
        )
    }

    /// The size in bytes of the cached item, if known.
    ///
    /// The length of the cached item may be unknown if the item is currently being streamed into
    /// the cache without a fixed length.
    pub fn known_length(&self) -> Option<u64> {
        self.handle.get_length()
    }

    /// The user-controlled metadata associated with the cached item.
    pub fn user_metadata(&self) -> Bytes {
        self.handle
            .get_user_metadata()
            .expect("`Found` is missing user_metadata")
            .clone()
    }

    /// Determines whether the cached item is usable.
    ///
    /// A cached item is usable if its age is less than the sum of the TTL and stale-while-revalidate
    /// periods.
    pub fn is_usable(&self) -> bool {
        self.handle.get_state().contains(CacheLookupState::USABLE)
    }

    /// Determines whether the cached item is stale.
    ///
    /// A cached item is stale if its age is greater than its TTL period.
    pub fn is_stale(&self) -> bool {
        self.handle.get_state().contains(CacheLookupState::STALE)
    }

    /// Determines the number of cache hits to this cached item.
    ///
    /// **Note**: this hit count only reflects the view of the server that supplied the cached
    /// item. Due to clustering, this count may vary between potentially many servers within the
    /// data center where the item is cached. See the [clustering
    /// documentation](https://developer.fastly.com/learning/vcl/clustering/) for details, though
    /// note that the exact caching architecture of Compute@Edge is different from VCL services.
    pub fn hits(&self) -> u64 {
        self.handle
            .get_hits()
            .expect("`Found` is missing hits metadata")
    }

    /// Retrieves the entire cached item as a [`Body`] that can be read in a streaming fashion.
    ///
    #[doc = include_str!("../../docs/snippets/cache-found-multiple-streams.md")]
    pub fn to_stream(&self) -> Result<Body, CacheError> {
        self.to_stream_from_range(None, None)
    }

    /// Retrieves a range of bytes from the cached item as a [`Body`] that can be read in a streaming fashion.
    ///
    /// If `from` is `None`, the stream will start from the beginning of the item. If `to` is
    /// `None`, the stream will end at the end of the item.
    ///
    /// If the provided range is invalid, the stream will contain the entire item. It is the
    /// caller's responsibility to check that the returned stream contains the number of bytes
    /// expected.
    ///
    #[doc = include_str!("../../docs/snippets/cache-found-multiple-streams.md")]
    pub fn to_stream_from_range(
        &self,
        from: Option<u64>,
        to: Option<u64>,
    ) -> Result<Body, CacheError> {
        let body_handle = self
            .handle
            .get_body(&GetBodyOptions { from, to })?
            .ok_or(CacheError::InvalidOperation)?;
        Ok(body_handle.into())
    }
}

/// An owned variant of `HandleWriteOptions`.
#[derive(Default)]
struct WriteOptions {
    max_age: Duration,
    request_headers: Option<RequestHandle>,
    /// A space-delimited list of headers to vary on
    vary_rule: Option<String>,
    initial_age: Option<Duration>,
    stale_while_revalidate: Option<Duration>,
    /// A space-delimited list of keys
    surrogate_keys: Option<String>,
    length: Option<u64>,
    user_metadata: Option<Bytes>,
    // Note: bool::default() == false
    sensitive_data: bool,
}

impl WriteOptions {
    fn as_handle_options(&self) -> HandleWriteOptions {
        let initial_age_ns = self.initial_age.map(|age| {
            age.as_nanos()
                .try_into()
                // We don't expect an initial age to use e.g. `Duration::MAX`, so panic
                // if the guest provides an unexpectedly large duration.
                .expect("initial_age larger than u64::MAX nanoseconds")
        });

        // By contrast to `initial_age`, it's plausible that guests would use
        // `Duration::MAX` for `max_age` or `stale_while_revalidate` to express
        // "unlimited" durations. Hence, we accept values larger than `u64::MAX`,
        // but truncate them -- a difference that cannot be observed, since that
        // value is > 500 years.
        let max_age_ns = self.max_age.as_nanos().try_into().unwrap_or(u64::MAX);
        let stale_while_revalidate_ns = self
            .stale_while_revalidate
            .map(|swr| swr.as_nanos().try_into().unwrap_or(u64::MAX));

        HandleWriteOptions {
            max_age_ns,
            request_headers: self.request_headers.as_ref(),
            vary_rule: self.vary_rule.as_deref(),
            initial_age_ns,
            stale_while_revalidate_ns,
            surrogate_keys: self.surrogate_keys.as_deref(),
            length: self.length,
            user_metadata: self.user_metadata.clone(),
            sensitive_data: self.sensitive_data,
        }
    }

    fn vary_by<'a>(&mut self, headers: impl IntoIterator<Item = &'a HeaderName>) {
        let mut vary_rule = String::new();
        for header in headers {
            if !vary_rule.is_empty() {
                vary_rule.push(' ')
            }
            vary_rule.push_str(header.as_str())
        }
        self.vary_rule = Some(vary_rule);
    }

    fn surrogate_keys<'a>(&mut self, surrogate_keys: impl IntoIterator<Item = &'a str>) {
        let mut keys = String::new();
        for key in surrogate_keys {
            if !keys.is_empty() {
                keys.push(' ')
            }
            keys.push_str(key);
        }
        self.surrogate_keys = Some(keys);
    }
}

/// A builder-style API for configuring a non-transactional insertion.
pub struct InsertBuilder {
    key: CacheKey,
    options: WriteOptions,
}

/// Returns an [`InsertBuilder`] that will perform a non-transactional cache insertion.
///
/// The required `ttl` argument is the "time to live" for the cache item: the time for which the
/// item will be considered fresh. All other insertion arguments are optional, and may be set using
/// the returned builder.
///
/// ```no_run
/// # use fastly::cache::core::*;
/// # use std::io::Write;
/// # use std::time::Duration;
/// let contents = b"my cached object";
/// let mut writer = insert(CacheKey::from_static(b"my_key"), Duration::from_secs(3600))
///     .surrogate_keys(["my_key"])
///     .known_length(contents.len() as u64)
///     .execute()
///     .unwrap();
/// writer.write_all(contents).unwrap();
/// writer.finish().unwrap();
/// ```
///
/// # Relationship with [`Transaction::lookup()`]
///
/// Like [`lookup()`], [`insert()`] may race with concurrent lookups or insertions, and will
/// unconditionally overwrite existing cached items rather than allowing for revalidation of an
/// existing object.
///
/// The transactional equivalent of this function is [`Transaction::insert()`], which may only be
/// called following a transactional lookup when [`Transaction::must_insert_or_update()`] returns
/// `true`.
pub fn insert(key: CacheKey, ttl: Duration) -> InsertBuilder {
    InsertBuilder {
        key,
        options: WriteOptions {
            max_age: ttl,
            ..WriteOptions::default()
        },
    }
}

impl InsertBuilder {
    /// Sets a multi-value header for this insertion, discarding any previous values associated
    /// with the header `name`.
    ///
    #[doc = include_str!("../../docs/snippets/cache-headers.md")]
    pub fn header_values<'a>(
        mut self,
        name: impl ToHeaderName,
        values: impl IntoIterator<Item = &'a HeaderValue>,
    ) -> Self {
        self.options
            .request_headers
            .get_or_insert_with(RequestHandle::new)
            .set_header_values(&name.into_owned(), values);
        self
    }

    /// Sets a single-value header for this insertion, discarding any previous values associated
    /// with the header `name`.
    ///
    #[doc = include_str!("../../docs/snippets/cache-headers.md")]
    pub fn header(self, name: impl ToHeaderName, value: impl ToHeaderValue) -> Self {
        self.header_values(&name.into_owned(), Some(&value.into_owned()))
    }

    /// Sets the list of headers that must match when looking up this cached item.
    ///
    #[doc = include_str!("../../docs/snippets/cache-headers.md")]
    pub fn vary_by<'a>(mut self, headers: impl IntoIterator<Item = &'a HeaderName>) -> Self {
        self.options.vary_by(headers);
        self
    }

    /// Sets the initial age of the cached item, to be used in freshness calculations.
    ///
    /// The initial age is `Duration::ZERO` by default.
    pub fn initial_age(mut self, age: Duration) -> Self {
        self.options.initial_age = Some(age);
        self
    }

    #[doc = include_str!("../../docs/snippets/cache-swr.md")]
    pub fn stale_while_revalidate(mut self, duration: Duration) -> Self {
        self.options.stale_while_revalidate = Some(duration);
        self
    }

    #[doc = include_str!("../../docs/snippets/cache-insert-surrogate-keys.md")]
    pub fn surrogate_keys<'a>(mut self, keys: impl IntoIterator<Item = &'a str>) -> Self {
        self.options.surrogate_keys(keys);
        self
    }

    #[doc = include_str!("../../docs/snippets/cache-insert-known-length.md")]
    pub fn known_length(mut self, length: u64) -> Self {
        self.options.length = Some(length);
        self
    }

    /// Sets the user-defined metadata to associate with the cached item.
    pub fn user_metadata(mut self, user_metadata: Bytes) -> Self {
        self.options.user_metadata = Some(user_metadata);
        self
    }

    #[doc = include_str!("../../docs/snippets/cache-insert-sensitive-data.md")]
    pub fn sensitive_data(mut self, is_sensitive_data: bool) -> Self {
        self.options.sensitive_data = is_sensitive_data;
        self
    }

    /// Begin the insertion, returning a [`StreamingBody`] for providing the cached object itself.
    ///
    #[doc = include_str!("../../docs/snippets/cache-insertion.md")]
    pub fn execute(self) -> Result<StreamingBody, CacheError> {
        let body_handle = handle::insert(self.key, &self.options.as_handle_options())?;
        Ok(body_handle.into())
    }
}

/// A cache transaction initiated by [`Transaction::lookup()`].
///
/// Transactions coordinate between concurrent actions on the same cache key, incorporating concepts
/// of [request collapsing](https://developer.fastly.com/learning/concepts/request-collapsing/) and
/// [revalidation](https://developer.fastly.com/learning/concepts/stale/), though at a lower level
/// that does not automatically interpret HTTP semantics.
///
/// # Request collapsing
///
/// If there are multiple concurrent calls to [`Transaction::lookup()`] for the same item and that
/// item is not present, just one of the callers will be instructed to insert the item into the
/// cache as part of the transaction. The other callers will block until the metadata for the item
/// has been inserted, and can then begin streaming its contents out of the cache at the same time
/// that the inserting caller streams them into the cache.
///
/// # Revalidation
///
/// Similarly, if an item is usable but stale, and multiple callers attempt a
/// [`Transaction::lookup()`] concurrently, they will all be given access to the stale item, but
/// only one will be designated to perform an asynchronous update (or insertion) to freshen the item
/// in the cache.
///
/// # Example
///
/// Users of the transactional API should at minimum anticipate lookups that are obligated to insert
/// an item into the cache, and lookups which are not. If the `stale-while-revalidate` parameter is
/// set for cached items, the user should also distinguish between the insertion and revalidation
/// cases.
///
/// ```no_run
/// # use fastly::cache::core::*;
/// # use std::io::Write;
/// # use std::time::Duration;
/// # fn build_contents() -> &'static [u8] { todo!() }
/// # fn use_found_item(_: &Found) {}
/// # fn should_replace(_: &Found, _: &'static [u8]) -> bool { todo!() }
/// const TTL: Duration = Duration::from_secs(3600);
/// // perform the lookup
/// let lookup_tx = Transaction::lookup(CacheKey::from_static(b"my_key"))
///     .execute()
///     .unwrap();
/// if let Some(found) = lookup_tx.found() {
///     // a cached item was found; we use it now even though it might be stale,
///     // and we'll revalidate it below
///     use_found_item(&found);
/// }
/// // now we need to handle the "must insert" and "must update" cases
/// if lookup_tx.must_insert() {
///     // a cached item was not found, and we've been chosen to insert it
///     let contents = build_contents();
///     let (mut writer, found) = lookup_tx
///         .insert(TTL)
///         .surrogate_keys(["my_key"])
///         .known_length(contents.len() as u64)
///         // stream back the object so we can use it after inserting
///         .execute_and_stream_back()
///         .unwrap();
///     writer.write_all(contents).unwrap();
///     writer.finish().unwrap();
///     // now we can use the item we just inserted
///     use_found_item(&found);
/// } else if lookup_tx.must_insert_or_update() {
///     // a cached item was found and used above, and now we need to perform
///     // revalidation
///     let revalidation_contents = build_contents();
///     if let Some(stale_found) = lookup_tx.found() {
///         if should_replace(&stale_found, &revalidation_contents) {
///             // use `insert` to replace the previous object
///             let mut writer = lookup_tx
///                 .insert(TTL)
///                 .surrogate_keys(["my_key"])
///                 .known_length(revalidation_contents.len() as u64)
///                 .execute()
///                 .unwrap();
///             writer.write_all(revalidation_contents).unwrap();
///             writer.finish().unwrap();
///         } else {
///             // otherwise update the stale object's metadata
///             lookup_tx
///                 .update(TTL)
///                 .surrogate_keys(["my_key"])
///                 .execute()
///                 .unwrap();
///         }
///     }
/// }
/// ```
pub struct Transaction {
    handle: Arc<CacheHandle>,
}

impl Transaction {
    /// Returns a [`TransactionLookupBuilder`] that will perform a transactional cache lookup.
    ///
    /// See [`Transaction`] for details and an example.
    pub fn lookup(key: CacheKey) -> TransactionLookupBuilder {
        TransactionLookupBuilder {
            key,
            options: LookupOptions::default(),
            lazy_await: false,
        }
    }

    /// Returns a `Found` object for this cache item, if one is available.
    ///
    /// Even if an object is found, the cache item might be stale and require updating. Use
    /// [`Transaction::must_insert_or_update()`] to determine whether this transaction client is
    /// expected to update the cached item.
    pub fn found(&self) -> Option<Found> {
        if self.handle.get_state().contains(CacheLookupState::FOUND) {
            Some(Found {
                handle: self.handle.clone(),
            })
        } else {
            None
        }
    }

    /// Returns `true` if a usable cached item was not found, and this transaction client is
    /// expected to insert one.
    ///
    /// Use [`Transaction::insert()`] to insert the cache item, or
    /// [`Transaction::cancel_insert_or_update()`] to exit the transaction without providing an
    /// item.
    pub fn must_insert(&self) -> bool {
        !self.handle.get_state().contains(CacheLookupState::FOUND)
            && self
                .handle
                .get_state()
                .contains(CacheLookupState::MUST_INSERT_OR_UPDATE)
    }

    /// Returns `true` if a fresh cache item was not found, and this transaction client is expected
    /// to insert a new item or update a stale item.
    ///
    /// Use:
    ///
    /// * [`Transaction::update()`] to freshen a found item by updating its metadata;
    /// * [`Transaction::insert()`] to insert a new item (including object data);
    /// * [`Transaction::cancel_insert_or_update()`] to exit the transaction without providing an item.
    pub fn must_insert_or_update(&self) -> bool {
        self.handle
            .get_state()
            .contains(CacheLookupState::MUST_INSERT_OR_UPDATE)
    }

    /// Cancels the obligation for this transaction client to insert or update a cache item.
    ///
    /// If there are concurrent transactional lookups that were blocked waiting on this client
    /// to provide the item, one of them will be chosen to be unblocked and given the
    /// [`Transaction::must_insert_or_update()`] obligation.
    ///
    /// This method should only be called when [`Transaction::must_insert_or_update()`] is true;
    /// otherwise, a [`CacheError::InvalidOperation`] will be returned.
    pub fn cancel_insert_or_update(&self) -> Result<(), CacheError> {
        Ok(self.handle.transaction_cancel()?)
    }

    /// Returns a [`TransactionInsertBuilder`] that will perform a transactional cache insertion.
    ///
    /// This method should only be called when [`Transaction::must_insert_or_update()`] is true;
    /// otherwise, a [`CacheError::InvalidOperation`] will be returned when attempting to execute
    /// the insertion.
    pub fn insert(self, ttl: Duration) -> TransactionInsertBuilder {
        TransactionInsertBuilder {
            handle: self.handle.clone(),
            options: WriteOptions {
                max_age: ttl,
                ..Default::default()
            },
        }
    }

    /// Returns a [`TransactionUpdateBuilder`] that will perform a transactional cache update.
    ///
    /// Updating an item freshens it by updating its metadata, e.g. its age, without changing the
    /// object itself.
    ///
    /// This method should only be called when [`Transaction::must_insert_or_update()`] is true
    /// _and_ the item is found (i.e. [`Transaction::found()`] is non-empty). Otherwise, a
    /// [`CacheError::InvalidOperation`] will be returned when attempting to execute the update.
    ///
    /// The method consumes the transaction. Call [`Transaction::found()`] before this method if
    /// subsequent access to the stale cached item is needed.
    ///
    /// **Important note**: the [`TransactionUpdateBuilder`] will replace _all_ of the configuration
    /// in the underlying cache item; if any configuration is not set on the builder, it will revert
    /// to the default value. So, for example, if a cached item previously had some surrogate keys
    /// set, and you want to retain them, you _must_ call
    /// [`TransactionUpdateBuilder::surrogate_keys()`] with the desired keys. Most configuration is
    /// available in the [`Found`] object.
    ///
    /// **Note**: the above behavior is likely to be replaced with defaulting the builder to the
    /// existing configuration, making it easier to retain the configuration by default. This change
    /// will be noted in a future changelog.
    pub fn update(self, ttl: Duration) -> TransactionUpdateBuilder {
        TransactionUpdateBuilder {
            handle: self.handle.clone(),
            options: WriteOptions {
                max_age: ttl,
                ..Default::default()
            },
        }
    }
}

/// A builder-style API for configuring a transactional lookup.
pub struct TransactionLookupBuilder {
    key: CacheKey,
    options: LookupOptions,
    // See the `lazy_await()` method
    lazy_await: bool,
}

impl TransactionLookupBuilder {
    /// Sets a multi-value header for this lookup, discarding any previous values associated
    /// with the header `name`.
    ///
    #[doc = include_str!("../../docs/snippets/cache-headers.md")]
    pub fn header_values<'a>(
        mut self,
        name: impl ToHeaderName,
        values: impl IntoIterator<Item = &'a HeaderValue>,
    ) -> Self {
        self.options
            .request_headers
            .get_or_insert_with(RequestHandle::new)
            .set_header_values(&name.into_owned(), values);
        self
    }

    /// Sets a single-value header for this lookup, discarding any previous values associated
    /// with the header `name`.
    ///
    #[doc = include_str!("../../docs/snippets/cache-headers.md")]
    pub fn header(self, name: impl ToHeaderName, value: impl ToHeaderValue) -> Self {
        self.header_values(&name.into_owned(), Some(&value.into_owned()))
    }

    /// An option used only for testing, which avoids forcing an await when executing the lookup, so
    /// that tests can take advantage of the platform's asynchrony.
    ///
    /// In the future, we'll provide a direct async SDK for transactions that will avoid the
    /// need for this flag.
    #[doc(hidden)]
    pub fn lazy_await(mut self) -> Self {
        self.lazy_await = true;
        self
    }

    /// Perform the lookup, entering a [`Transaction`].
    ///
    /// Accessors like [`Transaction::found()`] can be used to determine the outcome of the lookup.
    pub fn execute(self) -> Result<Transaction, CacheError> {
        let cache_handle = handle::transaction_lookup(self.key, &self.options.as_handle_options())?;
        // The underlying hostcall allows lookups to proceed asynchronously until "forced" to `await`
        // by another hostcall, such as an accessor. At the moment, we only provide a synchronous
        // interface to the low-level cache in the SDK, as we have not yet surfaced generic async
        // operations in the Rust SDK. Hence, we want to force the underlying `await` here, to eagerly
        // retrieve any errors with the lookup, which allows subsequent accessors to be infallible.
        //
        // In the future, we'll be able to provide an `async fn` version of `execute`, which will
        // clean this up. In the meantime, we have a hidden `lazy_await` field used purely for the
        // test suite, where a couple of tests rely on the underlying asynchrony in the platform.
        if !self.lazy_await {
            cache_handle.wait()?;
        }
        Ok(Transaction {
            handle: Arc::new(cache_handle),
        })
    }
}

/// A builder-style API for configuring a transactional cache insertion.
pub struct TransactionInsertBuilder {
    handle: Arc<CacheHandle>,
    options: WriteOptions,
}

impl TransactionInsertBuilder {
    /// Sets the list of headers that must match when looking up this cached item.
    ///
    #[doc = include_str!("../../docs/snippets/cache-headers.md")]
    pub fn vary_by<'a>(mut self, headers: impl IntoIterator<Item = &'a HeaderName>) -> Self {
        self.options.vary_by(headers);
        self
    }

    /// Sets the initial age of the cached item, to be used in freshness calculations.
    ///
    /// The initial age is `Duration::ZERO` by default.
    pub fn initial_age(mut self, age: Duration) -> Self {
        self.options.initial_age = Some(age);
        self
    }

    #[doc = include_str!("../../docs/snippets/cache-swr.md")]
    pub fn stale_while_revalidate(mut self, duration: Duration) -> Self {
        self.options.stale_while_revalidate = Some(duration);
        self
    }

    #[doc = include_str!("../../docs/snippets/cache-insert-surrogate-keys.md")]
    pub fn surrogate_keys<'a>(mut self, keys: impl IntoIterator<Item = &'a str>) -> Self {
        self.options.surrogate_keys(keys);
        self
    }

    #[doc = include_str!("../../docs/snippets/cache-insert-known-length.md")]
    pub fn known_length(mut self, length: u64) -> Self {
        self.options.length = Some(length);
        self
    }

    /// Sets the user-defined metadata to associate with the cached item.
    pub fn user_metadata(mut self, user_metadata: Bytes) -> Self {
        self.options.user_metadata = Some(user_metadata);
        self
    }

    #[doc = include_str!("../../docs/snippets/cache-insert-sensitive-data.md")]
    pub fn sensitive_data(mut self, is_sensitive_data: bool) -> Self {
        self.options.sensitive_data = is_sensitive_data;
        self
    }

    /// Begin the insertion, returning a [`StreamingBody`] for providing the cached object itself.
    ///
    #[doc = include_str!("../../docs/snippets/cache-insertion.md")]
    pub fn execute(self) -> Result<StreamingBody, CacheError> {
        let body_handle = self
            .handle
            .transaction_insert(&self.options.as_handle_options())?;
        Ok(body_handle.into())
    }

    /// Begin the insertion, and provide a `Found` object that can be used to stream out of the
    /// newly-inserted object.
    ///
    #[doc = include_str!("../../docs/snippets/cache-insertion.md")]
    ///
    /// The returned [`Found`] object allows the client inserting a cache item to efficiently read
    /// back the contents of that item, avoiding the need to buffer contents for copying to multiple
    /// destinations. This pattern is commonly required when caching an item that also must be
    /// provided to, e.g., the client response.
    pub fn execute_and_stream_back(self) -> Result<(StreamingBody, Found), CacheError> {
        let (body_handle, cache_handle) = self
            .handle
            .transaction_insert_and_stream_back(&self.options.as_handle_options())?;
        Ok((
            body_handle.into(),
            Found {
                handle: Arc::new(cache_handle),
            },
        ))
    }
}

/// A builder-style API for configuring a transactional cache update.
pub struct TransactionUpdateBuilder {
    handle: Arc<CacheHandle>,
    options: WriteOptions,
}

impl TransactionUpdateBuilder {
    /// Sets the list of headers that must match when looking up this cached item.
    ///
    #[doc = include_str!("../../docs/snippets/cache-headers.md")]
    pub fn vary_by<'a>(mut self, headers: impl IntoIterator<Item = &'a HeaderName>) -> Self {
        self.options.vary_by(headers);
        self
    }

    /// Sets the updated age of the cached item, to be used in freshness calculations.
    ///
    /// The updated age is `Duration::ZERO` by default.
    pub fn age(mut self, age: Duration) -> Self {
        self.options.initial_age = Some(age);
        self
    }

    #[doc = include_str!("../../docs/snippets/cache-swr.md")]
    pub fn stale_while_revalidate(mut self, duration: Duration) -> Self {
        self.options.stale_while_revalidate = Some(duration);
        self
    }

    #[doc = include_str!("../../docs/snippets/cache-insert-surrogate-keys.md")]
    pub fn surrogate_keys<'a>(mut self, keys: impl IntoIterator<Item = &'a str>) -> Self {
        self.options.surrogate_keys(keys);
        self
    }

    /// Sets the user-defined metadata to associate with the cached item.
    pub fn user_metadata(mut self, user_metadata: Bytes) -> Self {
        self.options.user_metadata = Some(user_metadata);
        self
    }

    #[doc = include_str!("../../docs/snippets/cache-insert-sensitive-data.md")]
    pub fn sensitive_data(mut self, is_sensitive_data: bool) -> Self {
        self.options.sensitive_data = is_sensitive_data;
        self
    }

    /// Perform the update of the cache item's metadata.
    pub fn execute(self) -> Result<(), CacheError> {
        let body_handle = self
            .handle
            .transaction_update(&self.options.as_handle_options())?;
        Ok(body_handle.into())
    }
}
