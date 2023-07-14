## 0.9.5 (2023-07-12)

### Added

- Added the Compute@Edge Simple Cache API in `fastly::cache::simple`.
- `fastly::error` now includes a reëxport of `anyhow::Context` in addition to `anyhow::Error`.
- Added `fastly::secret_store::Secret::from_bytes()` to create unencrypted `Secret`s from bytes at runtime for compatibility with APIs that require a `Secret`.

## 0.9.4 (2023-05-30)

### Added

- Added the Compute@Edge Core Cache API in `fastly::cache::core`.

### Changed

- Usage of `Backend::builder` is no longer considered experimental. The `BackendExt` trait is no longer required to use that function, and the trait function has been marked as deprecated.

### Deprecated

- Deprecated `BackendExt::builder` in favor of a native `Backend::builder` function.

## 0.9.3 (2023-05-15)

### Added

- `Request::get_client_request_id()` returns an identifier for the current client request.

### Changed

- Renamed Object Store to KV Store, and deprecated the previous names.
- The `std::io::Write` implementations for `BodyHandle` and `StreamingBodyHandle` no longer panic upon error.

### Deprecated

- Deprecated the old Object Store items that have been renamed to KV Store.
- Deprecated `write_bytes()` and `write_str()` methods for `BodyHandle` and `StreamingBodyHandle` in favor of `std::io::Write` methods.

## 0.9.2 (2023-03-23)

### Added

- Added various methods to `Backend` to access various configuration settings.

### Changed

- Adjusted documentation for `BackendBuilder::override_host`.
- Added examples to the module-level `limits` documentation.
- Bumped `bytes` dependency

### Fixed

- Updated documentation for `Response::stream_to_client()`, `Request::send_async_streaming()`, and `RequestHandle::send_async_streaming()` to show that streaming bodies must be `finish()`ed, not dropped.
- Secret store: `BUFLEN error` when the plaintext secret's length is greater than 599 bytes

## 0.9.1 (2023-01-18)

### Fixed

- Fixed warnings for unused internal items.

## 0.9.0 (2023-01-18)

### Added

- Added a new `fastly::secret_store` module for use with the new Fastly Secret Store.
- Added support for enabling or disabling reuse of connections for the experimental dynamic backend interface. The default is to reuse connections. Connection reuse is at the discretion of the server running the service.

### Changed

- Streaming bodies must now be explicitly `finish()`ed in order to be properly framed. The default behavior on drop for `StreamingBody` or `StreamingBodyHandle` now can yield an error to the recipient of the message (the client requesting the response, or the backend receiving a request). This is a breaking change for all current uses of these types, and prevents situations where errors or premature exits from C@E programs could cause incomplete `transfer-encoding: chunked` bodies to appear complete despite being truncated.
- `Request::with_header()` has been changed to have appending behavior rather than set behavior. As a result, chained invocations of `with_header()` will add multiple versions of the same header to the request, just like `Request::append_header()`. Users that prefer the old behavior should use `Request::with_set_header()`, instead.
- Documentation on backend creation has been extended, to make it clear how to tell if a service supports dynamic backends.
- The arguments to `BackendBuilder::new` and `BackendBuilder::override_host` have ben standardized to `impl ToString`, to match the other functions in the file.
- `LookupError` and `OpenError` enums for config stores are now `#[non_exhaustive]`.
- Improved error messages when some resource limits are exceeded.

### Removed

- Removed the migration guide Rustdoc for SDKs earlier than 0.6.0.
- Removed deprecated aliases from `Request`, `RequestHandle`, and `Response`.
- Removed the deprecated `legacy` items from `fastly-sys`.
- Removed deprecated aliases from `fastly-shared`.

## 0.8.9 (2022-10-20)

### Fixed

- Fixed a warning about an unused function.

## 0.8.8 (2022-10-18)

### Added

- Added `Request::get_tls_raw_client_certificate`, which returns the client's mutual TLS certificate.
- Added `Request::get_tls_client_cert_verify_result`, which returns an error code if applicable.
- Added an experimental `Request::handoff_websocket` interface, which hands off a WebSocket request to a backend.
- Added an experimental `Request::handoff_fanout` interface, which passes a request to the Fanout GRIP proxy.

### Changed

- Deprecated the experimental `RequestUpgradeWebsocket::upgrade_websocket` trait method.

### Fixed

- Fixed some documentation misspellings.
- Added an `http::purge` module, containing interfaces to purge surrogate keys. This was previously listed in the changelog of 0.8.7, but was not actually included in the release.

## 0.8.7 (2022-08-19)

### Added

- Added `Backend::is_healthy()` to `experimental` module
- Added `get_client_h2_fingerprint` to `Request` and `client_h2_fingerprint` to `RequestHandle` to get the HTTP/2 fingerprint of a client request if available
- Added an interface to the [Compute@Edge Object Store][object-store-blog].
- Added interfaces to issue [surrogate key purges][purge-docs].

[object-store-blog]: https://www.fastly.com/blog/introducing-the-compute-edge-object-store-global-persistent-storage-for-compute-functions
[purge-docs]: https://developer.fastly.com/learning/concepts/purging/

### Changed
- Improved deprecation messages for renaming of `Dictionary` to `ConfigStore`
- Fixed some misspellings.

## 0.8.6 (2022-06-22)

### Added

- Added an experimental dynamic backend interface.

### Changed

- Renamed Dictionaries to Config Stores, and deprecated the previous names.

## 0.8.5 (2022-05-03)

### Added

- Added `get_headers()` methods to `Request` and `Response` which return an iterator over all header values.
- Added `Request::get_tls_ja3_md5()` for getting the JA3 hash of the TLS ClientHello message.
- Added experimental WebSocket upgrade interface.

## 0.8.4 (2022-03-08)

### Added

- Added `set_framing_headers_mode` and `with_framing_headers_mode` methods to `Request` and `Response`, which allow you to manually control the `Content-Length` and `Transfer-Encoding` headers sent for a request or a response.

### Changed

- Switched to Rust 2021 edition, so the minimum supported Rust version is now 1.56.0.

## 0.8.3 (2022-02-23)

### Added

- Added `Body::try_get_prefix_mut` to allow handling I/O errors when reading a body prefix.

## 0.8.2 (2022-01-21)

### Added

- Added a set of `*_str_lossy()` accessors for HTTP headers and bodies. Unlike the `*_str` methods, they do not panic if the values contain invalid UTF-8, but may perform allocation to insert replacement characters for invalid sequences.

### Fixed

- Fixed a crash when using `Request::clone_with_body()`.

## 0.8.1 (2021-12-10)

### Added

- Added a `OpenError::DictionaryDoesNotExist` error variant, which identifies when a dictionary couldn't be found.
- Added `Dictionary::try_open()`, which returns a `Result<Dictionary, OpenError>`, allowing programs to explicitly handle open failures.
- Added automatic gzip decompression for backend responses; see `Request::set_auto_decompress_gzip()`.
- Added `Request::get_query_parameter()` for easy access to individual query parameter strings.
- Added `get_ttl()` and `get_stale_while_revalidate()` accessors for `CacheOverride`.

### Changed

- Renamed `with_body_bytes()` and `set_body_bytes()` methods to `with_body_octet_stream()` and `set_body_octet_stream()` to emphasize that they modify the `Content-Type` of the request or response. The original names are still present, but deprecated.
- When reading from an HTTP body, an unexpected EOF (e.g., if a backend disconnects) results in an appropriately-tagged `std::io::Error` rather than the generic `"fastly_http_body::read failed"`.

### Fixed

- Panics caused when request limits are exceeded in `Request::from_client()` or the `#[fastly::main]` macro now log a more informative error message; previously it was reported as `panicked at 'explicit panic'`.

## 0.8.0 (2021-09-01)

### Added

- Added `Dictionary::try_get`, which returns a `Result<String, LookupError>`, allowing programs to explicitly handle lookup failures.
- Added an `Other` variant to `dictionary::LookupError`.
- Added `close` to `RequestHandle`, and `ResponseHandle`.
- Added the `non_exhaustive` enums `HandleError` and `HandleKind` for the low level handle interface.
- Added `Satellite` and `UltraBroadband` variants for `ConnSpeed` and `ConnType` in the geolocation interface.
- Added `Other` variants for `ConnSpeed`, `ConnType`, `Continent`, `ProxyDescription`, and `ProxyType` in the geolocation interface for cases where the geolocation database contains variants that are newer than the current SDK definitions.

### Fixed

- Fixed geolocation calls returning no data when only partial data was available for a requested IP address.

### Changed

- The Minimum Supported Rust Version (MSRV) for the `fastly` crate is now 1.54.0.
- `BodyHandle::close` now works for non-streaming bodies in addition to the already-closeable streaming bodies.
- `BodyHandle`, `RequestHandle` and `ResponseHandle` now call `close` as part of their `Drop` implementation when they go out of scope, saving a small amount of leaked memory for services that make multiple HTTP requests.
- Exported unsafe low-level interfaces for creating `BodyHandle`s.
- The functions `is_valid` and `is_invalid` are now `const` for `BodyHandle`, `RequestHandle`, and `ResponseHandle`
- `Geo::utc_offset` now returns `Option<time::UtcOffset>` instead of `Option<chrono::FixedOffset>`.
- `ConnSpeed`, `ConnType`, `ProxyDescription`, and `ProxyType` are now [`non_exhaustive`](https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute).
- Exported the `fastly::dictionary` module. Its exported types `Dictionary` and `LookupError` remain accessible through other paths, but this module provides a way to import them together.

### Removed

- Removed `Copy` from `ConnSpeed`, `ConnType`, `Continent`, `ProxyDescription` and `ProxyType` inside the `geo` module; their new `Other` variants contain an arbitrary `String` which is not `Copy`able. The strings the `Other` variant might contain are small, so these enums (and `Geo` itself) can be efficiently cloned.

## 0.7.3 (2021-06-10)

### Fixed

- Removed the use of an unstable documentation feature that caused the `docs.rs` documentation build to fail.

## 0.7.2 (2021-06-10)

### Added

- Added an experimental API for controlling the cache keys used for requests. Note that experimental APIs are subject to change or removal even in minor versions of the SDK.
- Added two new error causes to `SendErrorCause`: `HeadTooLarge` and `InvalidStatus`. Previously these would be part of the `Invalid` variant, but now have their own to provide more insight as to what went wrong. In particular you will now know when a response fails due to an invalid status, such as `HTTP/1.1 42 MadeUpStatus` or if the response header was too large.

### Fixed

- Fixed a typo in a panic message inside of `fastly::handle::dictionary::DictionaryHandle::open`.

## 0.7.1 (2021-03-18)

### Fixed

- Fixed the buffer sizes reported in `BufferSizeError`s incorrectly reporting the initial buffer size rather than the maximum size the buffer can grow to. The maximum buffer size was still being used, but the error field was misleading.

- Dropped the unused dependency on `log`. See [`log-fastly`](https://docs.rs/log-fastly) for the recommended high-level logging interface.

## 0.7.0 (2021-03-03)

### Added

- Added `with_body_text_plain()`, `set_body_text_plain()`, `with_body_text_html()`,
  `set_body_text_html()` convenience methods which set the body contents along with their respective
  content types.
- Added `Response::see_other`, `Response::redirect`, and
  `Response::temporary_redirect` builders to support building a redirect
  response and its `Location` header all at once.
- Added `SendError::root_cause` and `SendErrorCause` to describe specific upstream request failure causes.

### Deprecated

- Deprecated `with_body_str()` and `set_body_str()` methods in favor of `with_body_text_plain()` and
  `set_body_text_plain()`.

### Changed

- `RequestHandle::send`, `RequestHandle::send_async`, and `Requestandle::send_async_streaming` now return a specific `SendErrorCause` on errors, replacing an `anyhow::Error`.
- `select_handles` now returns a specific `SendErrorCause` on errors, replacing an `anyhow::Error`.
- `PendingRequestHandle::poll` and `PendingRequestHandle::wait` now return a specific `SendErrorCause` on errors, replacing an `anyhow::Error`.

## 0.6.0 (2021-01-21)

### Added

- Added `Dictionary::contains` and `DictionaryHandle::contains` methods, which allow programs to check if a key exists in a Fastly Edge Dictionary.

### Changed

- Made a broad-ranging overhaul to the `Request` and `Response` APIs. See the [documentation of the `fastly` crate](https://docs.rs/fastly/0.6.0/fastly/) for details and a migration guide.

## 0.5.1 (2020-01-21)

### Changed

- Added an upper bound to the `fastly-sys` dependency to avoid conflicts with newer `fastly-sys` versions. We expect to address this by fixing `fastly-sys` semantic versioning in the future.

## 0.5.0 (2020-10-22)

### Added

- Added `fastly::dictionary::Dictionary`, which allows programs to look up values in Fastly Edge Dictionaries.

- Added `set_pci` method to `fastly::request::RequestExt` and `pci` to `fastly::request::RequestBuilderExt`, which both prevent cached content subject to compliance rules from being written to non-volatile storage.

- Added `set_surrogate_key` to `fastly::request::RequestExt` and `surrogate_key` to `fastly::request::RequestBuilderExt`. These allow surrogate keys to be added to cached content so that content may be purged in groups.

- Added `fastly::geo::Continent::as_code()` for easy access to two-letter continent codes.

### Changed

- `fastly::request::RequestExt` now offers `cache_override` and `cache_override_mut` as accessors to a `Request`'s `CacheOverride` instead of the `get_` and `set_` pair.

## 0.4.1 (2020-10-05)

### Fixed

- Fixed a `FixedOffset::east()` panic that could arise when handling geoip data when the geographic data for the IP address is invalid.

## 0.4.0 (2020-06-23)

### Added

- Added `get_header_value` method to `fastly::request::RequestHandle`.

- Added specific error types for some API calls:

  - `fastly::error::SendError` is returned by APIs that send backend requests. Note that the common case for a failed request remains an `Ok` result with a 5xx status code response.
  - `fastly::error::BufferSizeError` is returned by handle API calls that can fail due to an insufficient buffer size.

- Added `RequestExt::send_async_streaming()` and `RequestHandle::send_async_streaming()`, which allow programs to continue writing bytes to upstream request bodies after the headers have been sent.

- Added `Backend::name()` to get the string representation of a backend.

- Added `ResponseExt::backend()` to retrieve the `Backend` a response came from.

- Added `ResponseExt::backend_request()` and `ResponseExt::take_backend_request()` to retrieve the `Request` that this response was returned from, minus the body which is consumed when the request is sent. The `take` variant takes ownership of the `Request` so that it can be subsequently reused for another backend request.

- Added `PendingRequest::sent_req()` to retrieve the `Request` that was sent, minus the body which is consumed when the request is sent.

### Changed

- Removed `Result` return types from various functions and methods. Internal errors will now cause a panic. This primarily impacts the `Body`, `BodyHandle`, `RequestHandle`, and `ResponseHandle` types. This helps remove noise from `?` operators in cases where user programs cannot realistically recover from the error.

- `get_header_value` methods for `RequestHandle` and `ResponseHandle` will now return `Ok(None)` if the header does not exist, rather than an empty header.

- `Response::send_downstream()` and `ResponseHandle::send_downstream()` now begin sending the responses immediately, rather than when the program exits.

- Renamed `Backend::new()` to `Backend::from_name()`, and deprecated the old name.

### Deprecated

- Deprecated `Backend::new()` in favor of `Backend::from_name()`.

### Removed

- Removed `fastly::abi` submodule from the public interface.

- Removed `impl From<PendingRequestHandle> for PendingRequest`, as `PendingRequests` now must be build with the backend `Request` they were sent with.

## 0.3.3 (2020-05-21)

### Added

- Added `Drop` implementations for streaming bodies to close streaming responses when the associated `StreamingBodyHandle` or `StreamingBody` goes out of scope. This allows client requests to finish while the Compute@Edge program is still running.

- Added `downstream_original_header_count`, which gets the original number of headers of the downstream request.

- Added `ResponseHandle::remove_header` and `RequestHandle::remove_header`, which can remove headers directly from handles.

### Changed

- Separated the low-level Compute@Edge bindings into a new, separately-versioned crate, in order to reduce the frequency of breaking changes for users of the `fastly` crate.

## 0.3.2 (2020-05-09)

### Fixed

- Fixed a bug in the APIs which return iterators of values, such as `RequestHandle::get_header_values()`, that was causing the iterator to skip values when the buffer sizes were too small.

## 0.3.1 (2020-04-29)

### Added

- `downstream_client_ip_addr()` gets the IP address of the downstream client, when it is known.

- Geolocation information for IP addresses is now available in the `fastly::geo` module.

### Changed

- The `#[fastly::main]` attribute now can be applied to a function of any name, not just one called `main`.

## 0.3.0 (2020-04-16)

### Added

- Added the `#[fastly::main]` attribute to optionally reduce boilerplate in program entrypoints:

  ```rust
  #[fastly::main]
  fn main(ds_req: Request<Body>) -> Result<impl ResponseExt, Error> {
      ds_req.send("example_backend")
  }
  ```

- Added `downstream_tls_client_hello()` to get the raw bytes of the TLS ClientHello message.

- Added `downstream_original_header_names_with_len()` to get the request's header names as originally received, and in the original order they were received.

- Added `fastly::log::set_panic_endpoint()`, which lets you redirect Rust panic output to the logging endpoint of your choice.

### Changed

- Generalized the `Backend::send()` method to take any argument that implements the `fastly::RequestExt` trait.

### Removed

- Removed the dependency on the `regex` crate.

### Fixed

- Fixed validation for backend names, so that all valid backend names are now accepted. This was previously too conservative about what constitutes a valid backend name.

## 0.2.0-alpha4 (2020-04-08)

### Added

- Added APIs to override caching behavior of backend responses. This replaces the previous `ttl: i32` argument to `send()` and `send_async()`, and adds the ability to override `stale-while-revalidate`. See the `request::CacheOverride` type, as well as the new methods on `RequestExt` and the entirely new `RequestBuilderExt`.

- Added hostcall error code definitions to `XqdStatus`, and updated hostcall implementations to return these error codes.

- Added the `fastly::log` module, which contains a basic interface for writing to Fastly log endpoints.

## 0.2.0-alpha3 (2020-03-18)

### Added

- Added `request::downstream_tls_cipher_openssl_name()` and `request::downstream_tls_protocol()` to get basic TLS metadata for the downstream client request. These functions both return strings for the moment, but we will be evolving to more structured metadata in future releases.

- Added some checks to make sure backend requests are sent with complete URIs and valid backend names, returning with an error before trying to send if validation fails. Previously, this would fail outside of the WebAssembly program, making debugging more obscure.

- Added a `FromStr` implementation for `Backend`, allowing them to be `parse`d directly from a string. For example:

  ```rust
  let backend = "exampleOrigin".parse::<Backend>().unwrap();
  ```

### Changed

- Changed from blanket `RequestExt` and `ResponseExt` implementations for `AsRef<[u8]>` to implementations on specific concrete types. This includes a new implementation for `()` to represent an empty body, as well as all of the "stringy" types from the standard library like `String`, `&str`, `Vec<u8>`, and `&[u8]`.
