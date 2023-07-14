use fastly_shared::FastlyStatus;

use crate::{BodyHandle, RequestHandle};

pub type CacheHandle = u32;

pub type CacheObjectLength = u64;
pub type CacheDurationNs = u64;
pub type CacheHitCount = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct CacheLookupOptions {
    pub request_headers: RequestHandle,
}

bitflags::bitflags! {
    #[repr(transparent)]
    pub struct CacheLookupOptionsMask: u32 {
        const _RESERVED = 1 << 0;
        const REQUEST_HEADERS = 1 << 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct CacheWriteOptions {
    pub max_age_ns: u64,
    pub request_headers: RequestHandle,
    pub vary_rule_ptr: *const u8,
    pub vary_rule_len: usize,
    pub initial_age_ns: u64,
    pub stale_while_revalidate_ns: u64,
    pub surrogate_keys_ptr: *const u8,
    pub surrogate_keys_len: usize,
    pub length: CacheObjectLength,
    pub user_metadata_ptr: *const u8,
    pub user_metadata_len: usize,
}

bitflags::bitflags! {
    #[repr(transparent)]
    pub struct CacheWriteOptionsMask: u32 {
        const _RESERVED = 1 << 0;
        const REQUEST_HEADERS = 1 << 1;
        const VARY_RULE = 1 << 2;
        const INITIAL_AGE_NS = 1 << 3;
        const STALE_WHILE_REVALIDATE_NS = 1 << 4;
        const SURROGATE_KEYS = 1 << 5;
        const LENGTH = 1 << 6;
        const USER_METADATA = 1 << 7;
        const SENSITIVE_DATA = 1 << 8;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct CacheGetBodyOptions {
    pub from: u64,
    pub to: u64,
}

bitflags::bitflags! {
    #[repr(transparent)]
    pub struct CacheGetBodyOptionsMask: u32 {
        const _RESERVED = 1 << 0;
        const FROM = 1 << 1;
        const TO = 1 << 2;
    }
}

bitflags::bitflags! {
    #[repr(transparent)]
    pub struct CacheLookupState: u32 {
        const FOUND = 1 << 0;
        const USABLE = 1 << 1;
        const STALE = 1 << 2;
        const MUST_INSERT_OR_UPDATE = 1 << 3;
    }
}

#[link(wasm_import_module = "fastly_cache")]
extern "C" {
    #[link_name = "lookup"]
    pub fn lookup(
        cache_key_ptr: *const u8,
        cache_key_len: usize,
        options_mask: CacheLookupOptionsMask,
        options: *const CacheLookupOptions,
        cache_handle_out: *mut CacheHandle,
    ) -> FastlyStatus;

    #[link_name = "insert"]
    pub fn insert(
        cache_key_ptr: *const u8,
        cache_key_len: usize,
        options_mask: CacheWriteOptionsMask,
        options: *const CacheWriteOptions,
        body_handle_out: *mut BodyHandle,
    ) -> FastlyStatus;

    #[link_name = "transaction_lookup"]
    pub fn transaction_lookup(
        cache_key_ptr: *const u8,
        cache_key_len: usize,
        options_mask: CacheLookupOptionsMask,
        options: *const CacheLookupOptions,
        cache_handle_out: *mut CacheHandle,
    ) -> FastlyStatus;

    #[link_name = "transaction_insert"]
    pub fn transaction_insert(
        handle: CacheHandle,
        options_mask: CacheWriteOptionsMask,
        options: *const CacheWriteOptions,
        body_handle_out: *mut BodyHandle,
    ) -> FastlyStatus;

    #[link_name = "transaction_insert_and_stream_back"]
    pub fn transaction_insert_and_stream_back(
        handle: CacheHandle,
        options_mask: CacheWriteOptionsMask,
        options: *const CacheWriteOptions,
        body_handle_out: *mut BodyHandle,
        cache_handle_out: *mut CacheHandle,
    ) -> FastlyStatus;

    #[link_name = "transaction_update"]
    pub fn transaction_update(
        handle: CacheHandle,
        options_mask: CacheWriteOptionsMask,
        options: *const CacheWriteOptions,
    ) -> FastlyStatus;

    #[link_name = "transaction_cancel"]
    pub fn transaction_cancel(handle: CacheHandle) -> FastlyStatus;

    #[link_name = "close"]
    pub fn close(handle: CacheHandle) -> FastlyStatus;

    #[link_name = "get_state"]
    pub fn get_state(
        handle: CacheHandle,
        cache_lookup_state_out: *mut CacheLookupState,
    ) -> FastlyStatus;

    #[link_name = "get_user_metadata"]
    pub fn get_user_metadata(
        handle: CacheHandle,
        user_metadata_out_ptr: *mut u8,
        user_metadata_out_len: usize,
        nwritten_out: *mut usize,
    ) -> FastlyStatus;

    #[link_name = "get_body"]
    pub fn get_body(
        handle: CacheHandle,
        options_mask: CacheGetBodyOptionsMask,
        options: *const CacheGetBodyOptions,
        body_handle_out: *mut BodyHandle,
    ) -> FastlyStatus;

    #[link_name = "get_length"]
    pub fn get_length(handle: CacheHandle, length_out: *mut CacheObjectLength) -> FastlyStatus;

    #[link_name = "get_max_age_ns"]
    pub fn get_max_age_ns(handle: CacheHandle, duration_out: *mut CacheDurationNs) -> FastlyStatus;

    #[link_name = "get_stale_while_revalidate_ns"]
    pub fn get_stale_while_revalidate_ns(
        handle: CacheHandle,
        duration_out: *mut CacheDurationNs,
    ) -> FastlyStatus;

    #[link_name = "get_age_ns"]
    pub fn get_age_ns(handle: CacheHandle, duration_out: *mut CacheDurationNs) -> FastlyStatus;

    #[link_name = "get_hits"]
    pub fn get_hits(handle: CacheHandle, hits_out: *mut CacheHitCount) -> FastlyStatus;
}
