// TODO ACF 2020-12-01: remove once this is fixed: https://github.com/rust-lang/rust/issues/79581
#![allow(clashing_extern_declarations)]

//! FFI bindings to the Fastly Compute@Edge ABI.
//!
//! This is a low-level package; the [`fastly`](https://docs.rs/fastly) crate wraps these functions
//! in a much friendlier, Rust-like interface. You should not have to depend on this crate
//! explicitly in your `Cargo.toml`.
//!
//! # Versioning and compatibility
//!
//! The Cargo version of this package was previously set according to compatibility with the
//! Compute@Edge platform. Since the [`v0.25.0` release of the Fastly
//! CLI](https://github.com/fastly/cli/releases/tag/v0.25.0), the CLI is configured with the range
//! of `fastly-sys` versions that are currently compatible with the Compute@Edge platform. The Cargo
//! version of this package since `0.4.0` instead follows the [Cargo SemVer compatibility
//! guidelines](https://doc.rust-lang.org/cargo/reference/semver.html).
use fastly_shared::FastlyStatus;

pub mod fastly_cache;

// The following type aliases are used for readability of definitions in this module. They should
// not be confused with types of similar names in the `fastly` crate which are used to provide safe
// wrappers around these definitions.

pub type BodyHandle = u32;
pub type PendingRequestHandle = u32;
pub type RequestHandle = u32;
pub type ResponseHandle = u32;
pub type DictionaryHandle = u32;
#[deprecated(since = "0.9.3", note = "renamed to KV Store")]
pub type ObjectStoreHandle = u32;
pub type KVStoreHandle = u32;
pub type SecretStoreHandle = u32;
pub type SecretHandle = u32;
pub type AsyncItemHandle = u32;

#[repr(C)]
pub struct DynamicBackendConfig {
    pub host_override: *const u8,
    pub host_override_len: u32,
    pub connect_timeout_ms: u32,
    pub first_byte_timeout_ms: u32,
    pub between_bytes_timeout_ms: u32,
    pub ssl_min_version: u32,
    pub ssl_max_version: u32,
    pub cert_hostname: *const u8,
    pub cert_hostname_len: u32,
    pub ca_cert: *const u8,
    pub ca_cert_len: u32,
    pub ciphers: *const u8,
    pub ciphers_len: u32,
    pub sni_hostname: *const u8,
    pub sni_hostname_len: u32,
}

impl Default for DynamicBackendConfig {
    fn default() -> Self {
        DynamicBackendConfig {
            host_override: std::ptr::null(),
            host_override_len: 0,
            connect_timeout_ms: 0,
            first_byte_timeout_ms: 0,
            between_bytes_timeout_ms: 0,
            ssl_min_version: 0,
            ssl_max_version: 0,
            cert_hostname: std::ptr::null(),
            cert_hostname_len: 0,
            ca_cert: std::ptr::null(),
            ca_cert_len: 0,
            ciphers: std::ptr::null(),
            ciphers_len: 0,
            sni_hostname: std::ptr::null(),
            sni_hostname_len: 0,
        }
    }
}

bitflags::bitflags! {
    /// `Content-Encoding` codings.
    ///
    /// This type must match the definition in `typename.witx`, and will be replaced once we
    /// generate Wasm-side bindings from witx.
    #[derive(Default)]
    #[repr(transparent)]
    pub struct ContentEncodings: u32 {
        const GZIP = 1 << 0;
    }
}

bitflags::bitflags! {
    /// `BackendConfigOptions` codings.
    ///
    /// We are crossing our fingers that witx+wiggle define these in order in the obvious way,
    /// and that our test suite will sort out any incompatibilities. This is not great.
    #[derive(Default)]
    #[repr(transparent)]
    pub struct BackendConfigOptions: u32 {
        const RESERVED = 1 << 0;
        const HOST_OVERRIDE = 1 << 1;
        const CONNECT_TIMEOUT = 1 << 2;
        const FIRST_BYTE_TIMEOUT = 1 << 3;
        const BETWEEN_BYTES_TIMEOUT = 1 << 4;
        const USE_SSL = 1 << 5;
        const SSL_MIN_VERSION = 1 << 6;
        const SSL_MAX_VERSION = 1 << 7;
        const CERT_HOSTNAME = 1 << 8;
        const CA_CERT = 1 << 9;
        const CIPHERS = 1 << 10;
        const SNI_HOSTNAME = 1 << 11;
        const DONT_POOL = 1 << 12;
    }
}

pub mod fastly_abi {
    use super::*;

    #[link(wasm_import_module = "fastly_abi")]
    extern "C" {
        #[link_name = "init"]
        /// Tell the runtime what ABI version this program is using (FASTLY_ABI_VERSION)
        pub fn init(abi_version: u64) -> FastlyStatus;
    }
}

pub mod fastly_uap {
    use super::*;

    #[link(wasm_import_module = "fastly_uap")]
    extern "C" {
        #[link_name = "parse"]
        pub fn parse(
            user_agent: *const u8,
            user_agent_max_len: usize,
            family: *mut u8,
            family_max_len: usize,
            family_written: *mut usize,
            major: *mut u8,
            major_max_len: usize,
            major_written: *mut usize,
            minor: *mut u8,
            minor_max_len: usize,
            minor_written: *mut usize,
            patch: *mut u8,
            patch_max_len: usize,
            patch_written: *mut usize,
        ) -> FastlyStatus;
    }
}

pub mod fastly_http_body {
    use super::*;

    #[link(wasm_import_module = "fastly_http_body")]
    extern "C" {
        #[link_name = "append"]
        pub fn append(dst_handle: BodyHandle, src_handle: BodyHandle) -> FastlyStatus;

        #[link_name = "new"]
        pub fn new(handle_out: *mut BodyHandle) -> FastlyStatus;

        #[link_name = "read"]
        pub fn read(
            body_handle: BodyHandle,
            buf: *mut u8,
            buf_len: usize,
            nread_out: *mut usize,
        ) -> FastlyStatus;

        // overeager warning for extern declarations is a rustc bug: https://github.com/rust-lang/rust/issues/79581
        #[allow(clashing_extern_declarations)]
        #[link_name = "write"]
        pub fn write(
            body_handle: BodyHandle,
            buf: *const u8,
            buf_len: usize,
            end: fastly_shared::BodyWriteEnd,
            nwritten_out: *mut usize,
        ) -> FastlyStatus;

        /// Close a body, freeing its resources and causing any sends to finish.
        #[link_name = "close"]
        pub fn close(body_handle: BodyHandle) -> FastlyStatus;
    }
}

pub mod fastly_log {
    use super::*;

    #[link(wasm_import_module = "fastly_log")]
    extern "C" {
        #[link_name = "endpoint_get"]
        pub fn endpoint_get(
            name: *const u8,
            name_len: usize,
            endpoint_handle_out: *mut u32,
        ) -> FastlyStatus;

        // overeager warning for extern declarations is a rustc bug: https://github.com/rust-lang/rust/issues/79581
        #[allow(clashing_extern_declarations)]
        #[link_name = "write"]
        pub fn write(
            endpoint_handle: u32,
            msg: *const u8,
            msg_len: usize,
            nwritten_out: *mut usize,
        ) -> FastlyStatus;

    }
}

pub mod fastly_http_req {
    use super::*;

    #[link(wasm_import_module = "fastly_http_req")]
    extern "C" {
        #[link_name = "body_downstream_get"]
        pub fn body_downstream_get(
            req_handle_out: *mut RequestHandle,
            body_handle_out: *mut BodyHandle,
        ) -> FastlyStatus;

        #[link_name = "cache_override_set"]
        pub fn cache_override_set(
            req_handle: RequestHandle,
            tag: u32,
            ttl: u32,
            swr: u32,
        ) -> FastlyStatus;

        #[link_name = "cache_override_v2_set"]
        pub fn cache_override_v2_set(
            req_handle: RequestHandle,
            tag: u32,
            ttl: u32,
            swr: u32,
            sk: *const u8,
            sk_len: usize,
        ) -> FastlyStatus;

        #[link_name = "framing_headers_mode_set"]
        pub fn framing_headers_mode_set(
            req_handle: RequestHandle,
            mode: fastly_shared::FramingHeadersMode,
        ) -> FastlyStatus;

        #[link_name = "downstream_client_ip_addr"]
        pub fn downstream_client_ip_addr(
            addr_octets_out: *mut u8,
            nwritten_out: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "downstream_client_h2_fingerprint"]
        pub fn downstream_client_h2_fingerprint(
            h2fp_out: *mut u8,
            h2fp_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "downstream_client_request_id"]
        pub fn downstream_client_request_id(
            reqid_out: *mut u8,
            reqid_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "downstream_tls_cipher_openssl_name"]
        pub fn downstream_tls_cipher_openssl_name(
            cipher_out: *mut u8,
            cipher_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "downstream_tls_protocol"]
        pub fn downstream_tls_protocol(
            protocol_out: *mut u8,
            protocol_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "downstream_tls_client_hello"]
        pub fn downstream_tls_client_hello(
            client_hello_out: *mut u8,
            client_hello_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "downstream_tls_ja3_md5"]
        pub fn downstream_tls_ja3_md5(
            ja3_md5_out: *mut u8,
            nwritten_out: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "downstream_tls_raw_client_certificate"]
        pub fn downstream_tls_raw_client_certificate(
            client_hello_out: *mut u8,
            client_hello_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "downstream_tls_client_cert_verify_result"]
        pub fn downstream_tls_client_cert_verify_result(
            verify_result_out: *mut u32,
        ) -> FastlyStatus;

        #[link_name = "header_append"]
        pub fn header_append(
            req_handle: RequestHandle,
            name: *const u8,
            name_len: usize,
            value: *const u8,
            value_len: usize,
        ) -> FastlyStatus;

        #[link_name = "header_insert"]
        pub fn header_insert(
            req_handle: RequestHandle,
            name: *const u8,
            name_len: usize,
            value: *const u8,
            value_len: usize,
        ) -> FastlyStatus;

        #[link_name = "original_header_names_get"]
        pub fn original_header_names_get(
            buf: *mut u8,
            buf_len: usize,
            cursor: u32,
            ending_cursor: *mut i64,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "original_header_count"]
        pub fn original_header_count(count_out: *mut u32) -> FastlyStatus;

        #[link_name = "header_names_get"]
        pub fn header_names_get(
            req_handle: RequestHandle,
            buf: *mut u8,
            buf_len: usize,
            cursor: u32,
            ending_cursor: *mut i64,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "header_values_get"]
        pub fn header_values_get(
            req_handle: RequestHandle,
            name: *const u8,
            name_len: usize,
            buf: *mut u8,
            buf_len: usize,
            cursor: u32,
            ending_cursor: *mut i64,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "header_values_set"]
        pub fn header_values_set(
            req_handle: RequestHandle,
            name: *const u8,
            name_len: usize,
            values: *const u8,
            values_len: usize,
        ) -> FastlyStatus;

        #[link_name = "header_value_get"]
        pub fn header_value_get(
            req_handle: RequestHandle,
            name: *const u8,
            name_len: usize,
            value: *mut u8,
            value_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "header_remove"]
        pub fn header_remove(
            req_handle: RequestHandle,
            name: *const u8,
            name_len: usize,
        ) -> FastlyStatus;

        #[link_name = "method_get"]
        pub fn method_get(
            req_handle: RequestHandle,
            method: *mut u8,
            method_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "method_set"]
        pub fn method_set(
            req_handle: RequestHandle,
            method: *const u8,
            method_len: usize,
        ) -> FastlyStatus;

        #[link_name = "new"]
        pub fn new(req_handle_out: *mut RequestHandle) -> FastlyStatus;

        #[link_name = "send"]
        pub fn send(
            req_handle: RequestHandle,
            body_handle: BodyHandle,
            backend: *const u8,
            backend_len: usize,
            resp_handle_out: *mut ResponseHandle,
            resp_body_handle_out: *mut BodyHandle,
        ) -> FastlyStatus;

        #[link_name = "send_async"]
        pub fn send_async(
            req_handle: RequestHandle,
            body_handle: BodyHandle,
            backend: *const u8,
            backend_len: usize,
            pending_req_handle_out: *mut PendingRequestHandle,
        ) -> FastlyStatus;

        #[link_name = "send_async_streaming"]
        pub fn send_async_streaming(
            req_handle: RequestHandle,
            body_handle: BodyHandle,
            backend: *const u8,
            backend_len: usize,
            pending_req_handle_out: *mut PendingRequestHandle,
        ) -> FastlyStatus;

        #[link_name = "upgrade_websocket"]
        pub fn upgrade_websocket(backend: *const u8, backend_len: usize) -> FastlyStatus;

        #[link_name = "redirect_to_websocket_proxy"]
        pub fn redirect_to_websocket_proxy(backend: *const u8, backend_len: usize) -> FastlyStatus;

        #[link_name = "redirect_to_grip_proxy"]
        pub fn redirect_to_grip_proxy(backend: *const u8, backend_len: usize) -> FastlyStatus;

        #[link_name = "register_dynamic_backend"]
        pub fn register_dynamic_backend(
            name_prefix: *const u8,
            name_prefix_len: usize,
            target: *const u8,
            target_len: usize,
            config_mask: BackendConfigOptions,
            config: *const DynamicBackendConfig,
        ) -> FastlyStatus;

        #[link_name = "uri_get"]
        pub fn uri_get(
            req_handle: RequestHandle,
            uri: *mut u8,
            uri_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "uri_set"]
        pub fn uri_set(req_handle: RequestHandle, uri: *const u8, uri_len: usize) -> FastlyStatus;

        #[link_name = "version_get"]
        pub fn version_get(req_handle: RequestHandle, version: *mut u32) -> FastlyStatus;

        #[link_name = "version_set"]
        pub fn version_set(req_handle: RequestHandle, version: u32) -> FastlyStatus;

        #[link_name = "pending_req_poll"]
        pub fn pending_req_poll(
            pending_req_handle: PendingRequestHandle,
            is_done_out: *mut i32,
            resp_handle_out: *mut ResponseHandle,
            resp_body_handle_out: *mut BodyHandle,
        ) -> FastlyStatus;

        #[link_name = "pending_req_select"]
        pub fn pending_req_select(
            pending_req_handles: *const PendingRequestHandle,
            pending_req_handles_len: usize,
            done_index_out: *mut i32,
            resp_handle_out: *mut ResponseHandle,
            resp_body_handle_out: *mut BodyHandle,
        ) -> FastlyStatus;

        #[link_name = "pending_req_wait"]
        pub fn pending_req_wait(
            pending_req_handle: PendingRequestHandle,
            resp_handle_out: *mut ResponseHandle,
            resp_body_handle_out: *mut BodyHandle,
        ) -> FastlyStatus;

        #[link_name = "fastly_key_is_valid"]
        pub fn fastly_key_is_valid(is_valid_out: *mut u32) -> FastlyStatus;

        #[link_name = "close"]
        pub fn close(req_handle: RequestHandle) -> FastlyStatus;

        #[link_name = "auto_decompress_response_set"]
        pub fn auto_decompress_response_set(
            req_handle: RequestHandle,
            encodings: ContentEncodings,
        ) -> FastlyStatus;
    }
}

pub mod fastly_http_resp {
    use super::*;

    #[link(wasm_import_module = "fastly_http_resp")]
    extern "C" {
        #[link_name = "header_append"]
        pub fn header_append(
            resp_handle: ResponseHandle,
            name: *const u8,
            name_len: usize,
            value: *const u8,
            value_len: usize,
        ) -> FastlyStatus;

        #[link_name = "header_insert"]
        pub fn header_insert(
            resp_handle: ResponseHandle,
            name: *const u8,
            name_len: usize,
            value: *const u8,
            value_len: usize,
        ) -> FastlyStatus;

        #[link_name = "header_names_get"]
        pub fn header_names_get(
            resp_handle: ResponseHandle,
            buf: *mut u8,
            buf_len: usize,
            cursor: u32,
            ending_cursor: *mut i64,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "header_value_get"]
        pub fn header_value_get(
            resp_handle: ResponseHandle,
            name: *const u8,
            name_len: usize,
            value: *mut u8,
            value_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "header_values_get"]
        pub fn header_values_get(
            resp_handle: ResponseHandle,
            name: *const u8,
            name_len: usize,
            buf: *mut u8,
            buf_len: usize,
            cursor: u32,
            ending_cursor: *mut i64,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "header_values_set"]
        pub fn header_values_set(
            resp_handle: ResponseHandle,
            name: *const u8,
            name_len: usize,
            values: *const u8,
            values_len: usize,
        ) -> FastlyStatus;

        #[link_name = "header_remove"]
        pub fn header_remove(
            resp_handle: ResponseHandle,
            name: *const u8,
            name_len: usize,
        ) -> FastlyStatus;

        #[link_name = "new"]
        pub fn new(resp_handle_out: *mut ResponseHandle) -> FastlyStatus;

        #[link_name = "send_downstream"]
        pub fn send_downstream(
            resp_handle: ResponseHandle,
            body_handle: BodyHandle,
            streaming: u32,
        ) -> FastlyStatus;

        #[link_name = "status_get"]
        pub fn status_get(resp_handle: ResponseHandle, status: *mut u16) -> FastlyStatus;

        #[link_name = "status_set"]
        pub fn status_set(resp_handle: ResponseHandle, status: u16) -> FastlyStatus;

        #[link_name = "version_get"]
        pub fn version_get(resp_handle: ResponseHandle, version: *mut u32) -> FastlyStatus;

        #[link_name = "version_set"]
        pub fn version_set(resp_handle: ResponseHandle, version: u32) -> FastlyStatus;

        #[link_name = "framing_headers_mode_set"]
        pub fn framing_headers_mode_set(
            resp_handle: ResponseHandle,
            mode: fastly_shared::FramingHeadersMode,
        ) -> FastlyStatus;

        #[doc(hidden)]
        #[link_name = "http_keepalive_mode_set"]
        pub fn http_keepalive_mode_set(
            resp_handle: ResponseHandle,
            mode: fastly_shared::HttpKeepaliveMode,
        ) -> FastlyStatus;

        #[link_name = "close"]
        pub fn close(resp_handle: ResponseHandle) -> FastlyStatus;
    }
}

pub mod fastly_dictionary {
    use super::*;

    #[link(wasm_import_module = "fastly_dictionary")]
    extern "C" {
        #[link_name = "open"]
        pub fn open(
            name: *const u8,
            name_len: usize,
            dict_handle_out: *mut DictionaryHandle,
        ) -> FastlyStatus;

        #[link_name = "get"]
        pub fn get(
            dict_handle: DictionaryHandle,
            key: *const u8,
            key_len: usize,
            value: *mut u8,
            value_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;
    }
}

pub mod fastly_geo {
    use super::*;

    #[link(wasm_import_module = "fastly_geo")]
    extern "C" {
        #[link_name = "lookup"]
        pub fn lookup(
            addr_octets: *const u8,
            addr_len: usize,
            buf: *mut u8,
            buf_len: usize,
            nwritten_out: *mut usize,
        ) -> FastlyStatus;
    }
}

#[deprecated(since = "0.9.3", note = "renamed to KV Store")]
pub use fastly_kv_store as fastly_object_store;

pub mod fastly_kv_store {
    use super::*;

    // TODO ACF 2023-04-11: keep the object store name here until the ABI is updated
    #[link(wasm_import_module = "fastly_object_store")]
    extern "C" {
        #[link_name = "open"]
        pub fn open(
            name_ptr: *const u8,
            name_len: usize,
            kv_store_handle_out: *mut KVStoreHandle,
        ) -> FastlyStatus;

        #[link_name = "lookup"]
        pub fn lookup(
            kv_store_handle: KVStoreHandle,
            key_ptr: *const u8,
            key_len: usize,
            body_handle_out: *mut BodyHandle,
        ) -> FastlyStatus;

        #[link_name = "insert"]
        pub fn insert(
            kv_store_handle: KVStoreHandle,
            key_ptr: *const u8,
            key_len: usize,
            body_handle: BodyHandle,
        ) -> FastlyStatus;
    }
}

pub mod fastly_secret_store {
    use super::*;

    #[link(wasm_import_module = "fastly_secret_store")]
    extern "C" {
        #[link_name = "open"]
        pub fn open(
            secret_store_name_ptr: *const u8,
            secret_store_name_len: usize,
            secret_store_handle_out: *mut SecretStoreHandle,
        ) -> FastlyStatus;

        #[link_name = "get"]
        pub fn get(
            secret_store_handle: SecretStoreHandle,
            secret_name_ptr: *const u8,
            secret_name_len: usize,
            secret_handle_out: *mut SecretHandle,
        ) -> FastlyStatus;

        #[link_name = "plaintext"]
        pub fn plaintext(
            secret_handle: SecretHandle,
            plaintext_buf: *mut u8,
            plaintext_max_len: usize,
            nwritten_out: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "from_bytes"]
        pub fn from_bytes(
            plaintext_buf: *const u8,
            plaintext_len: usize,
            secret_handle_out: *mut SecretHandle,
        ) -> FastlyStatus;
    }
}

pub mod fastly_backend {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(u32)]
    pub enum BackendHealth {
        Unknown,
        Healthy,
        Unhealthy,
    }

    #[link(wasm_import_module = "fastly_backend")]
    extern "C" {
        #[link_name = "exists"]
        pub fn exists(
            backend_ptr: *const u8,
            backend_len: usize,
            backend_exists_out: *mut u32,
        ) -> FastlyStatus;

        #[link_name = "is_healthy"]
        pub fn is_healthy(
            backend_ptr: *const u8,
            backend_len: usize,
            backend_health_out: *mut BackendHealth,
        ) -> FastlyStatus;

        #[link_name = "is_dynamic"]
        pub fn is_dynamic(
            backend_ptr: *const u8,
            backend_len: usize,
            value: *mut u32,
        ) -> FastlyStatus;

        #[link_name = "get_host"]
        pub fn get_host(
            backend_ptr: *const u8,
            backend_len: usize,
            value: *mut u8,
            value_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "get_override_host"]
        pub fn get_override_host(
            backend_ptr: *const u8,
            backend_len: usize,
            value: *mut u8,
            value_max_len: usize,
            nwritten: *mut usize,
        ) -> FastlyStatus;

        #[link_name = "get_port"]
        pub fn get_port(
            backend_ptr: *const u8,
            backend_len: usize,
            value: *mut u16,
        ) -> FastlyStatus;

        #[link_name = "get_connect_timeout_ms"]
        pub fn get_connect_timeout_ms(
            backend_ptr: *const u8,
            backend_len: usize,
            value: *mut u32,
        ) -> FastlyStatus;

        #[link_name = "get_first_byte_timeout_ms"]
        pub fn get_first_byte_timeout_ms(
            backend_ptr: *const u8,
            backend_len: usize,
            value: *mut u32,
        ) -> FastlyStatus;

        #[link_name = "get_between_bytes_timeout_ms"]
        pub fn get_between_bytes_timeout_ms(
            backend_ptr: *const u8,
            backend_len: usize,
            value: *mut u32,
        ) -> FastlyStatus;

        #[link_name = "is_ssl"]
        pub fn is_ssl(backend_ptr: *const u8, backend_len: usize, value: *mut u32) -> FastlyStatus;

        #[link_name = "get_ssl_min_version"]
        pub fn get_ssl_min_version(
            backend_ptr: *const u8,
            backend_len: usize,
            value: *mut u32,
        ) -> FastlyStatus;

        #[link_name = "get_ssl_max_version"]
        pub fn get_ssl_max_version(
            backend_ptr: *const u8,
            backend_len: usize,
            value: *mut u32,
        ) -> FastlyStatus;
    }
}

pub mod fastly_async_io {
    use super::*;

    #[link(wasm_import_module = "fastly_async_io")]
    extern "C" {
        #[link_name = "select"]
        pub fn select(
            async_item_handles: *const AsyncItemHandle,
            async_item_handles_len: usize,
            timeout_ms: u32,
            done_index_out: *mut u32,
        ) -> FastlyStatus;

        #[link_name = "is_ready"]
        pub fn is_ready(async_item_handle: AsyncItemHandle, ready_out: *mut u32) -> FastlyStatus;
    }
}

pub mod fastly_purge {
    use super::*;

    bitflags::bitflags! {
        #[derive(Default)]
        #[repr(transparent)]
        pub struct PurgeOptionsMask: u32 {
            const SOFT_PURGE = 1 << 0;
            const RET_BUF = 1 << 1;
        }
    }

    #[derive(Debug)]
    #[repr(C)]
    pub struct PurgeOptions {
        pub ret_buf_ptr: *mut u8,
        pub ret_buf_len: usize,
        pub ret_buf_nwritten_out: *mut usize,
    }

    #[link(wasm_import_module = "fastly_purge")]
    extern "C" {
        #[link_name = "purge_surrogate_key"]
        pub fn purge_surrogate_key(
            surrogate_key_ptr: *const u8,
            surrogate_key_len: usize,
            options_mask: PurgeOptionsMask,
            options: *mut PurgeOptions,
        ) -> FastlyStatus;
    }
}
