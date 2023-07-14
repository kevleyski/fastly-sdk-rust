//! Experimental Compute@Edge features.
use crate::{
    abi::{self, FastlyStatus},
    http::{
        header::{HeaderName, HeaderValue},
        request::{
            handle::redirect_to_grip_proxy, handle::redirect_to_websocket_proxy,
            handle::RequestHandle, CacheKeyGen, Request, SendError, SendErrorCause,
        },
        response::assert_single_downstream_response_is_sent,
    },
    Backend, Error,
};
use anyhow::anyhow;
use fastly_sys::fastly_backend;
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[doc(inline)]
pub use fastly_sys::fastly_backend::BackendHealth;

pub use crate::backend::{BackendBuilder, BackendCreationError};

/// Parse a user agent string.
#[doc = include_str!("../docs/snippets/experimental.md")]
pub fn uap_parse(
    user_agent: &str,
) -> Result<(String, Option<String>, Option<String>, Option<String>), Error> {
    let user_agent: &[u8] = user_agent.as_ref();
    let max_length = 255;
    let mut family = Vec::with_capacity(max_length);
    let mut major = Vec::with_capacity(max_length);
    let mut minor = Vec::with_capacity(max_length);
    let mut patch = Vec::with_capacity(max_length);
    let mut family_nwritten = 0;
    let mut major_nwritten = 0;
    let mut minor_nwritten = 0;
    let mut patch_nwritten = 0;

    let status = unsafe {
        abi::fastly_uap::parse(
            user_agent.as_ptr(),
            user_agent.len(),
            family.as_mut_ptr(),
            family.capacity(),
            &mut family_nwritten,
            major.as_mut_ptr(),
            major.capacity(),
            &mut major_nwritten,
            minor.as_mut_ptr(),
            minor.capacity(),
            &mut minor_nwritten,
            patch.as_mut_ptr(),
            patch.capacity(),
            &mut patch_nwritten,
        )
    };
    if status.is_err() {
        return Err(Error::msg("fastly_uap::parse failed"));
    }
    assert!(
        family_nwritten <= family.capacity(),
        "fastly_uap::parse wrote too many bytes for family"
    );
    unsafe {
        family.set_len(family_nwritten);
    }
    assert!(
        major_nwritten <= major.capacity(),
        "fastly_uap::parse wrote too many bytes for major"
    );
    unsafe {
        major.set_len(major_nwritten);
    }
    assert!(
        minor_nwritten <= minor.capacity(),
        "fastly_uap::parse wrote too many bytes for minor"
    );
    unsafe {
        minor.set_len(minor_nwritten);
    }
    assert!(
        patch_nwritten <= patch.capacity(),
        "fastly_uap::parse wrote too many bytes for patch"
    );
    unsafe {
        patch.set_len(patch_nwritten);
    }
    Ok((
        String::from_utf8_lossy(&family).to_string(),
        Some(String::from_utf8_lossy(&major).to_string()),
        Some(String::from_utf8_lossy(&minor).to_string()),
        Some(String::from_utf8_lossy(&patch).to_string()),
    ))
}

/// An extension trait for [`Request`]s that adds methods for controlling cache keys.
#[doc = include_str!("../docs/snippets/experimental.md")]
pub trait RequestCacheKey {
    /// See [`Request::set_cache_key()`].
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn set_cache_key(&mut self, key: [u8; 32]);
    /// See [`Request::with_cache_key()`].
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn with_cache_key(self, key: [u8; 32]) -> Self;
    /// See [`Request::set_cache_key_fn()`].
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn set_cache_key_fn(&mut self, f: impl Fn(&Request) -> [u8; 32] + Send + Sync + 'static);
    /// See [`Request::with_cache_key_fn()`].
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn with_cache_key_fn(self, f: impl Fn(&Request) -> [u8; 32] + Send + Sync + 'static) -> Self;
    /// See [`Request::set_cache_key_str()`].
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn set_cache_key_str(&mut self, key_str: impl AsRef<[u8]>);
    /// See [`Request::with_cache_key_str()`].
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn with_cache_key_str(self, key_str: impl AsRef<[u8]>) -> Self;
}

impl RequestCacheKey for Request {
    /// Set the cache key to be used when attempting to satisfy this request from a cached response.
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn set_cache_key(&mut self, key: [u8; 32]) {
        self.cache_key = Some(CacheKeyGen::Set(key));
    }

    /// Builder-style equivalent of [`set_cache_key()`](Self::set_cache_key()).
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn with_cache_key(mut self, key: [u8; 32]) -> Self {
        self.set_cache_key(key);
        self
    }

    /// Set the function that will be used to compute the cache key for this request.
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn set_cache_key_fn(&mut self, f: impl Fn(&Request) -> [u8; 32] + Send + Sync + 'static) {
        self.cache_key = Some(CacheKeyGen::Lazy(Arc::new(f)));
    }

    /// Builder-style equivalent of [`set_cache_key_fn()`](Self::set_cache_key_fn()).
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn with_cache_key_fn(
        mut self,
        f: impl Fn(&Request) -> [u8; 32] + Send + Sync + 'static,
    ) -> Self {
        self.set_cache_key_fn(f);
        self
    }

    /// Set a string as the cache key to be used when attempting to satisfy this request from a
    /// cached response.
    ///
    /// The string representation of the key is hashed to the same `[u8; 32]` representation used by
    /// [`set_cache_key()`][`Self::set_cache_key()`].
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn set_cache_key_str(&mut self, key_str: impl AsRef<[u8]>) {
        let mut sha = Sha256::new();
        sha.update(key_str);
        sha.update(b"\x00\xf0\x9f\xa7\x82\x00"); // extra salt
        self.set_cache_key(*sha.finalize().as_ref())
    }

    /// Builder-style equivalent of [`set_cache_key_str()`](Self::set_cache_key_str()).
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn with_cache_key_str(mut self, key_str: impl AsRef<[u8]>) -> Self {
        self.set_cache_key_str(key_str);
        self
    }
}

/// An extension trait for [`RequestHandle`](RequestHandle)s that adds methods for controlling cache
/// keys.
#[doc = include_str!("../docs/snippets/experimental.md")]
pub trait RequestHandleCacheKey {
    /// See [`RequestHandle::set_cache_key()`](RequestHandle::set_cache_key).
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn set_cache_key(&mut self, key: &[u8; 32]);
}

impl RequestHandleCacheKey for RequestHandle {
    /// Set the cache key to be used when attempting to satisfy this request from a cached response.
    #[doc = include_str!("../docs/snippets/experimental.md")]
    fn set_cache_key(&mut self, key: &[u8; 32]) {
        const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
        let mut hex = [0; 64];
        for (i, b) in key.iter().enumerate() {
            hex[i * 2] = DIGITS[(b >> 4) as usize];
            hex[i * 2 + 1] = DIGITS[(b & 0xf) as usize];
        }

        self.insert_header(
            &HeaderName::from_static("fastly-xqd-cache-key"),
            &HeaderValue::from_bytes(&hex).unwrap(),
        )
    }
}

/// An extension trait for [`Request`](Request)s that adds a method for upgrading websockets.
pub trait RequestUpgradeWebsocket {
    /// See [`Request::handoff_websocket()`].
    fn handoff_websocket(self, backend: &str) -> Result<(), SendError>;

    /// See [`Request::handoff_fanout()`].
    fn handoff_fanout(self, backend: &str) -> Result<(), SendError>;
}
impl RequestUpgradeWebsocket for Request {
    /// Pass the WebSocket directly to a backend.
    ///
    /// This can only be used on services that have the WebSockets feature enabled and on requests
    /// that are valid WebSocket requests.
    ///
    /// The sending completes in the background. Once this method has been called, no other
    /// response can be sent to this request, and the application can exit without affecting the
    /// send.
    fn handoff_websocket(self, backend: &str) -> Result<(), SendError> {
        assert_single_downstream_response_is_sent(true);
        let status = redirect_to_websocket_proxy(backend);
        if status.is_err() {
            Err(SendError::new(
                backend,
                self,
                SendErrorCause::status(status),
            ))
        } else {
            Ok(())
        }
    }

    /// Pass the request through the Fanout GRIP proxy and on to a backend.
    ///
    /// This can only be used on services that have the Fanout feature enabled.
    ///
    /// The sending completes in the background. Once this method has been called, no other
    /// response can be sent to this request, and the application can exit without affecting the
    /// send.
    fn handoff_fanout(self, backend: &str) -> Result<(), SendError> {
        assert_single_downstream_response_is_sent(true);
        let status = redirect_to_grip_proxy(backend);
        if status.is_err() {
            Err(SendError::new(
                backend,
                self,
                SendErrorCause::status(status),
            ))
        } else {
            Ok(())
        }
    }
}

/// An extension trait for [`RequestHandle`](RequestHandle)s that adds methods for upgrading
/// websockets.
pub trait RequestHandleUpgradeWebsocket {
    /// See [`RequestHandle::handoff_websocket()`].
    fn handoff_websocket(&mut self, backend: &str) -> Result<(), SendErrorCause>;

    /// See [`RequestHandle::handoff_fanout()`].
    fn handoff_fanout(&mut self, backend: &str) -> Result<(), SendErrorCause>;
}

impl RequestHandleUpgradeWebsocket for RequestHandle {
    /// Pass the WebSocket directly to a backend.
    ///
    /// This can only be used on services that have the WebSockets feature enabled and on requests
    /// that are valid WebSocket requests.
    ///
    /// The sending completes in the background. Once this method has been called, no other
    /// response can be sent to this request, and the application can exit without affecting the
    /// send.
    fn handoff_websocket(&mut self, backend: &str) -> Result<(), SendErrorCause> {
        match unsafe {
            abi::fastly_http_req::redirect_to_websocket_proxy(backend.as_ptr(), backend.len())
        } {
            FastlyStatus::OK => Ok(()),
            status => Err(SendErrorCause::status(status)),
        }
    }

    /// Pass the request through the Fanout GRIP proxy and on to a backend.
    ///
    /// This can only be used on services that have the Fanout feature enabled.
    ///
    /// The sending completes in the background. Once this method has been called, no other
    /// response can be sent to this request, and the application can exit without affecting the
    /// send.
    fn handoff_fanout(&mut self, backend: &str) -> Result<(), SendErrorCause> {
        match unsafe {
            abi::fastly_http_req::redirect_to_grip_proxy(backend.as_ptr(), backend.len())
        } {
            FastlyStatus::OK => Ok(()),
            status => Err(SendErrorCause::status(status)),
        }
    }
}

/// An extension trait for experimental [`Backend`] methods.
pub trait BackendExt {
    #[deprecated(
        since = "0.9.3",
        note = "The BackendExt::builder trait method is now part of Backend."
    )]
    #[doc = include_str!("../docs/snippets/dynamic-backend-builder.md")]
    fn builder(name: impl ToString, target: impl ToString) -> BackendBuilder;

    /// Return the health of the backend if configured and currently known.
    ///
    /// For backends without a configured healthcheck, this will always return `Unknown`.
    fn is_healthy(&self) -> Result<BackendHealth, Error>;
}

impl BackendExt for Backend {
    fn builder(name: impl ToString, target: impl ToString) -> BackendBuilder {
        BackendBuilder::new(name.to_string(), target.to_string())
    }

    fn is_healthy(&self) -> Result<BackendHealth, Error> {
        let mut backend_health_out = BackendHealth::Unknown;
        unsafe {
            fastly_backend::is_healthy(
                self.name().as_ptr(),
                self.name().len(),
                &mut backend_health_out,
            )
        }
        .result()
        .map_err(|e| anyhow!("backend healthcheck error: {:?}", e))?;
        Ok(backend_health_out)
    }
}
