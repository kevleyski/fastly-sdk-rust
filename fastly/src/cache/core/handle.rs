use bytes::{Bytes, BytesMut};
use fastly_shared::{FastlyStatus, INVALID_CACHE_HANDLE};
use fastly_sys::fastly_cache::{self as sys, CacheHitCount};
pub use fastly_sys::fastly_cache::{CacheDurationNs, CacheLookupState, CacheObjectLength};
use std::ptr;

use crate::handle::{BodyHandle, RequestHandle, StreamingBodyHandle};

/// A cache key consists of up to 4KiB of arbitrary bytes.
pub type CacheKey = Bytes;

#[derive(Debug, Default)]
pub struct LookupOptions<'a> {
    pub request_headers: Option<&'a RequestHandle>,
}

impl<'a> LookupOptions<'a> {
    fn as_abi(&self) -> (sys::CacheLookupOptionsMask, sys::CacheLookupOptions) {
        use sys::CacheLookupOptionsMask as Mask;

        let mut mask = Mask::empty();
        let request_headers = if let Some(v) = &self.request_headers {
            mask.insert(Mask::REQUEST_HEADERS);
            v.as_u32()
        } else {
            RequestHandle::INVALID.as_u32()
        };
        let options = sys::CacheLookupOptions { request_headers };
        (mask, options)
    }
}

#[derive(Debug, Default)]
pub struct WriteOptions<'a> {
    pub max_age_ns: u64,
    pub request_headers: Option<&'a RequestHandle>,
    pub vary_rule: Option<&'a str>,
    pub initial_age_ns: Option<u64>,
    pub stale_while_revalidate_ns: Option<u64>,
    pub surrogate_keys: Option<&'a str>,
    pub length: Option<CacheObjectLength>,
    pub user_metadata: Option<Bytes>,
    pub sensitive_data: bool,
}

impl<'a> WriteOptions<'a> {
    fn as_abi(&self) -> (sys::CacheWriteOptionsMask, sys::CacheWriteOptions) {
        use sys::CacheWriteOptionsMask as Mask;

        let mut mask = Mask::empty();
        let request_headers = if let Some(v) = &self.request_headers {
            mask.insert(Mask::REQUEST_HEADERS);
            v.as_u32()
        } else {
            RequestHandle::INVALID.as_u32()
        };
        let (vary_rule_ptr, vary_rule_len) = if let Some(v) = self.vary_rule {
            mask.insert(Mask::VARY_RULE);
            (v.as_ptr(), v.len())
        } else {
            (ptr::null(), 0)
        };
        let initial_age_ns = if let Some(v) = self.initial_age_ns {
            mask.insert(Mask::INITIAL_AGE_NS);
            v
        } else {
            0
        };
        let stale_while_revalidate_ns = if let Some(v) = self.stale_while_revalidate_ns {
            mask.insert(Mask::STALE_WHILE_REVALIDATE_NS);
            v
        } else {
            0
        };
        let (surrogate_keys_ptr, surrogate_keys_len) = if let Some(v) = self.surrogate_keys {
            mask.insert(Mask::SURROGATE_KEYS);
            (v.as_ptr(), v.len())
        } else {
            (ptr::null(), 0)
        };
        let length = if let Some(v) = self.length {
            mask.insert(Mask::LENGTH);
            v
        } else {
            0
        };
        let (user_metadata_ptr, user_metadata_len) = if let Some(v) = &self.user_metadata {
            mask.insert(Mask::USER_METADATA);
            (v.as_ptr(), v.len())
        } else {
            (ptr::null(), 0)
        };
        if self.sensitive_data {
            mask.insert(Mask::SENSITIVE_DATA);
        }
        let options = sys::CacheWriteOptions {
            max_age_ns: self.max_age_ns,
            request_headers,
            vary_rule_ptr,
            vary_rule_len,
            initial_age_ns,
            stale_while_revalidate_ns,
            surrogate_keys_ptr,
            surrogate_keys_len,
            length,
            user_metadata_ptr,
            user_metadata_len,
        };
        (mask, options)
    }
}

#[derive(Debug, Default)]
pub struct GetBodyOptions {
    pub from: Option<u64>,
    pub to: Option<u64>,
}

impl GetBodyOptions {
    fn as_abi(&self) -> (sys::CacheGetBodyOptionsMask, sys::CacheGetBodyOptions) {
        use sys::CacheGetBodyOptionsMask as Mask;

        let mut mask = Mask::empty();
        let from = if let Some(v) = self.from {
            mask.insert(Mask::FROM);
            v
        } else {
            0
        };
        let to = if let Some(v) = self.to {
            mask.insert(Mask::TO);
            v
        } else {
            0
        };
        let options = sys::CacheGetBodyOptions { from, to };
        (mask, options)
    }
}

pub fn lookup(key: CacheKey, options: &LookupOptions) -> Result<CacheHandle, FastlyStatus> {
    let mut cache_handle_out = CacheHandle::INVALID;
    let (options_mask, options) = options.as_abi();
    unsafe {
        sys::lookup(
            key.as_ptr(),
            key.len(),
            options_mask,
            &options,
            cache_handle_out.as_abi_mut(),
        )
    }
    .result()?;
    Ok(cache_handle_out)
}

pub fn insert(key: CacheKey, options: &WriteOptions) -> Result<StreamingBodyHandle, FastlyStatus> {
    let mut body_handle_out = BodyHandle::INVALID;
    let (options_mask, options) = options.as_abi();
    unsafe {
        sys::insert(
            key.as_ptr(),
            key.len(),
            options_mask,
            &options,
            body_handle_out.as_u32_mut(),
        )
    }
    .result()?;
    Ok(StreamingBodyHandle::from_body_handle(body_handle_out))
}

pub fn transaction_lookup(
    key: CacheKey,
    options: &LookupOptions,
) -> Result<CacheHandle, FastlyStatus> {
    let mut cache_handle_out = CacheHandle::INVALID;
    let (options_mask, options) = options.as_abi();
    unsafe {
        sys::transaction_lookup(
            key.as_ptr(),
            key.len(),
            options_mask,
            &options,
            cache_handle_out.as_abi_mut(),
        )
    }
    .result()?;
    Ok(cache_handle_out)
}

pub struct CacheHandle {
    cache_handle: sys::CacheHandle,
}

impl Drop for CacheHandle {
    fn drop(&mut self) {
        // Only try to close on drop if the handle is not invalid
        if !self.is_invalid() {
            unsafe {
                sys::close(self.as_abi());
            }
        }
    }
}

impl CacheHandle {
    const INVALID: Self = CacheHandle {
        cache_handle: INVALID_CACHE_HANDLE,
    };

    fn is_invalid(&self) -> bool {
        self.cache_handle == INVALID_CACHE_HANDLE
    }

    fn as_abi(&self) -> sys::CacheHandle {
        self.cache_handle
    }

    fn as_abi_mut(&mut self) -> &mut sys::CacheHandle {
        &mut self.cache_handle
    }

    pub fn transaction_insert(
        &self,
        options: &WriteOptions,
    ) -> Result<StreamingBodyHandle, FastlyStatus> {
        let mut body_handle_out = BodyHandle::INVALID;
        let (options_mask, options) = options.as_abi();
        unsafe {
            sys::transaction_insert(
                self.as_abi(),
                options_mask,
                &options,
                body_handle_out.as_u32_mut(),
            )
        }
        .result()?;
        Ok(StreamingBodyHandle::from_body_handle(body_handle_out))
    }

    pub fn transaction_insert_and_stream_back(
        &self,
        options: &WriteOptions,
    ) -> Result<(StreamingBodyHandle, CacheHandle), FastlyStatus> {
        let mut body_handle_out = BodyHandle::INVALID;
        let mut cache_handle_out = CacheHandle::INVALID;
        let (options_mask, options) = options.as_abi();
        unsafe {
            sys::transaction_insert_and_stream_back(
                self.as_abi(),
                options_mask,
                &options,
                body_handle_out.as_u32_mut(),
                cache_handle_out.as_abi_mut(),
            )
        }
        .result()?;
        let streaming_body_handle = StreamingBodyHandle::from_body_handle(body_handle_out);
        Ok((streaming_body_handle, cache_handle_out))
    }

    pub fn transaction_update(&self, options: &WriteOptions) -> Result<(), FastlyStatus> {
        let (options_mask, options) = options.as_abi();
        unsafe { sys::transaction_update(self.as_abi(), options_mask, &options) }.result()
    }

    pub fn transaction_cancel(&self) -> Result<(), FastlyStatus> {
        unsafe { sys::transaction_cancel(self.as_abi()) }.result()
    }

    /// Transactions are internally asynchronous, but we don't yet fully expose that in the SDK.
    /// The internal asynchrony means that lookup errors can be deferred. Rather than making
    /// all accessors failable, we provide this method for forcing the underlying await and returning
    /// any error, after which accessors should be guaranteed to succeed without panicking.
    pub(crate) fn wait(&self) -> Result<(), FastlyStatus> {
        // use the `get_state` hostcall as an arbitrary choice of hostcall that will force the
        // await and surface any errors.
        let mut cache_lookup_state_out = CacheLookupState::empty();
        unsafe { sys::get_state(self.as_abi(), &mut cache_lookup_state_out) }.result()
    }

    pub fn get_state(&self) -> CacheLookupState {
        let mut cache_lookup_state_out = CacheLookupState::empty();
        unsafe { sys::get_state(self.as_abi(), &mut cache_lookup_state_out) }
            .result()
            .expect("sys::get_state failed");
        cache_lookup_state_out
    }

    pub fn get_user_metadata(&self) -> Option<Bytes> {
        const INITIAL_CAPACITY: usize = 16 * 1024;
        let mut user_metadata = BytesMut::with_capacity(INITIAL_CAPACITY);
        let mut nwritten_out = 0;
        let status = unsafe {
            sys::get_user_metadata(
                self.as_abi(),
                user_metadata.as_mut_ptr(),
                user_metadata.capacity(),
                &mut nwritten_out,
            )
        };
        match status {
            FastlyStatus::OK => {
                // Resize the byte string to the amount that was written out, and return it.
                unsafe { user_metadata.set_len(nwritten_out) };
                return Some(user_metadata.freeze());
            }
            FastlyStatus::NONE => {
                // No user metadata; handle is likely not in the `FOUND` state
                return None;
            }
            FastlyStatus::BUFLEN => {
                // The first attempt for user metadata may "fail" with a BUFLEN if the user metadata
                // is larger than `INITIAL_CAPACITY`, but the length we need will be written to
                // `nwritten_out`. This code path continues in the remainder of this function.
            }
            status => {
                // any other errors are an SDK bug; panic
                panic!("sys::get_user_metadata failed with {status:?}");
            }
        }
        // The length of `user_metadata` should be zero, and so it should be fine to just reserve
        // the amount returned in nwritten_out, but the subtraction guards against that assumption
        // changing.
        user_metadata.reserve(nwritten_out - user_metadata.len());
        unsafe {
            sys::get_user_metadata(
                self.as_abi(),
                user_metadata.as_mut_ptr(),
                user_metadata.capacity(),
                &mut nwritten_out,
            )
        }
        .result()
        .expect("sys::get_user_metadata failed");
        unsafe { user_metadata.set_len(nwritten_out) };
        Some(user_metadata.freeze())
    }

    pub fn get_body(&self, options: &GetBodyOptions) -> Result<Option<BodyHandle>, FastlyStatus> {
        let mut body_handle_out = BodyHandle::INVALID;
        let (options_mask, options) = options.as_abi();
        let status = unsafe {
            sys::get_body(
                self.as_abi(),
                options_mask,
                &options,
                body_handle_out.as_u32_mut(),
            )
        };
        match status {
            FastlyStatus::OK => Ok(Some(body_handle_out)),
            FastlyStatus::NONE => Ok(None),
            status => Err(status),
        }
    }

    pub fn get_length(&self) -> Option<CacheObjectLength> {
        let mut length_out = 0;
        let status = unsafe { sys::get_length(self.as_abi(), &mut length_out) };
        match status {
            FastlyStatus::OK => Some(length_out),
            FastlyStatus::NONE => None,
            status => {
                // any other errors are an SDK bug; panic
                panic!("sys::get_length failed with {status:?}");
            }
        }
    }

    pub fn get_max_age_ns(&self) -> Option<CacheDurationNs> {
        let mut duration_out = 0;
        let status = unsafe { sys::get_max_age_ns(self.as_abi(), &mut duration_out) };
        match status {
            FastlyStatus::OK => Some(duration_out),
            FastlyStatus::NONE => None,
            status => {
                // any other errors are an SDK bug; panic
                panic!("sys::get_max_age_ns failed with {status:?}");
            }
        }
    }

    pub fn get_stale_while_revalidate_ns(&self) -> Option<CacheDurationNs> {
        let mut duration_out = 0;
        let status =
            unsafe { sys::get_stale_while_revalidate_ns(self.as_abi(), &mut duration_out) };
        match status {
            FastlyStatus::OK => Some(duration_out),
            FastlyStatus::NONE => None,
            status => {
                // any other errors are an SDK bug; panic
                panic!("sys::get_stale_while_revalidate_ns failed with {status:?}");
            }
        }
    }

    pub fn get_age_ns(&self) -> Option<CacheDurationNs> {
        let mut duration_out = 0;
        let status = unsafe { sys::get_age_ns(self.as_abi(), &mut duration_out) };
        match status {
            FastlyStatus::OK => Some(duration_out),
            FastlyStatus::NONE => None,
            status => {
                // any other errors are an SDK bug; panic
                panic!("sys::get_age_ns failed with {status:?}");
            }
        }
    }

    pub fn get_hits(&self) -> Option<CacheHitCount> {
        let mut hits_out = 0;
        let status = unsafe { sys::get_hits(self.as_abi(), &mut hits_out) };
        match status {
            FastlyStatus::OK => Some(hits_out),
            FastlyStatus::NONE => None,
            status => {
                // any other errors are an SDK bug; panic
                panic!("sys::get_hits failed with {status:?}");
            }
        }
    }
}
