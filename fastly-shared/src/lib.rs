// Warnings (other than unused variables) in doctests are promoted to errors.
#![doc(test(attr(deny(warnings))))]
#![doc(test(attr(allow(dead_code))))]
#![doc(test(attr(allow(unused_variables))))]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::invalid_codeblock_attributes)]

use std::fmt;

use http::HeaderValue;

/// The maximum number of pending requests that can be passed to `select`.
///
/// In practice, a program will be limited first by the number of requests it can create.
pub const MAX_PENDING_REQS: u32 = 16 * 1024;

// These should always be a very high number that is not `MAX`, to avoid clashing with both
// legitimate handles, as well as other sentinel values defined by cranelift_entity.
pub const INVALID_REQUEST_HANDLE: u32 = std::u32::MAX - 1;
pub const INVALID_PENDING_REQUEST_HANDLE: u32 = std::u32::MAX - 1;
pub const INVALID_RESPONSE_HANDLE: u32 = std::u32::MAX - 1;
pub const INVALID_BODY_HANDLE: u32 = std::u32::MAX - 1;
pub const INVALID_DICTIONARY_HANDLE: u32 = std::u32::MAX - 1;
#[deprecated(since = "0.9.3", note = "renamed to KV Store")]
pub const INVALID_OBJECT_STORE_HANDLE: u32 = INVALID_KV_STORE_HANDLE;
pub const INVALID_KV_STORE_HANDLE: u32 = std::u32::MAX - 1;
pub const INVALID_SECRET_STORE_HANDLE: u32 = std::u32::MAX - 1;
pub const INVALID_SECRET_HANDLE: u32 = std::u32::MAX - 1;
pub const INVALID_CACHE_HANDLE: u32 = std::u32::MAX - 1;

/// Constants for defining minimum/maximum TLS versions for connecting to backends.
#[allow(non_snake_case)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SslVersion {
    TLS1 = 0,
    TLS1_1 = 1,
    TLS1_2 = 2,
    TLS1_3 = 3,
}

impl SslVersion {
    pub fn as_u32(&self) -> u32 {
        *self as u32
    }
}

// TODO KTM 2023-02-08: could use num-derive for this, but I don't think it's worth pulling in a
// whole new set of dependencies when this will likely be encoded by witx shortly (see HttpVersion)
impl TryFrom<u32> for SslVersion {
    type Error = String;
    fn try_from(x: u32) -> Result<Self, Self::Error> {
        if x == Self::TLS1 as u32 {
            Ok(Self::TLS1)
        } else if x == Self::TLS1_1 as u32 {
            Ok(Self::TLS1_1)
        } else if x == Self::TLS1_2 as u32 {
            Ok(Self::TLS1_2)
        } else if x == Self::TLS1_3 as u32 {
            Ok(Self::TLS1_3)
        } else {
            Err(format!("unknown ssl version enum value: {}", x))
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(transparent)]
pub struct FastlyStatus {
    pub code: i32,
}

impl FastlyStatus {
    /// Success value.
    ///
    /// This indicates that a hostcall finished successfully.
    pub const OK: Self = Self { code: 0 };
    /// Generic error value.
    ///
    /// This means that some unexpected error occurred during a hostcall.
    pub const ERROR: Self = Self { code: 1 };
    /// Invalid argument.
    pub const INVAL: Self = Self { code: 2 };
    /// Invalid handle.
    ///
    /// Returned when a request, response, or body handle is not valid.
    pub const BADF: Self = Self { code: 3 };
    /// Buffer length error.
    ///
    /// Returned when a buffer is too long.
    pub const BUFLEN: Self = Self { code: 4 };
    /// Unsupported operation error.
    ///
    /// This error is returned when some operation cannot be performed, because it is not supported.
    pub const UNSUPPORTED: Self = Self { code: 5 };
    /// Alignment error.
    ///
    /// This is returned when a pointer does not point to a properly aligned slice of memory.
    pub const BADALIGN: Self = Self { code: 6 };
    /// Invalid HTTP error.
    ///
    /// This can be returned when a method, URI, or header is not valid.
    pub const HTTPINVALID: Self = Self { code: 7 };
    /// HTTP user error.
    ///
    /// This is returned in cases where user code caused an HTTP error. For example, attempt to send
    /// a 1xx response code, or a request with a non-absolute URI. This can also be caused by
    /// an unexpected header: both `content-length` and `transfer-encoding`, for example.
    pub const HTTPUSER: Self = Self { code: 8 };
    /// HTTP incomplete message error.
    ///
    /// This can be returned when a stream ended unexpectedly.
    pub const HTTPINCOMPLETE: Self = Self { code: 9 };
    /// A `None` error.
    ///
    /// This status code is used to indicate when an optional value did not exist, as opposed to
    /// an empty value.
    pub const NONE: Self = Self { code: 10 };
    /// HTTP head too large error.
    ///
    /// This error will be returned when the message head is too large.
    pub const HTTPHEADTOOLARGE: Self = Self { code: 11 };
    /// HTTP invalid status error.
    ///
    /// This error will be returned when the HTTP message contains an invalid status code.
    pub const HTTPINVALIDSTATUS: Self = Self { code: 12 };
    /// Limit exceeded
    ///
    /// This is returned when an attempt to allocate a resource has exceeded the maximum number of
    /// resources permitted. For example, creating too many response handles.
    pub const LIMITEXCEEDED: Self = Self { code: 13 };

    pub fn is_ok(&self) -> bool {
        self == &Self::OK
    }

    pub fn is_err(&self) -> bool {
        !self.is_ok()
    }

    /// Convert a `FastlyStatus` value to a `Result<(), FastlyStatus>`.
    ///
    /// This will consume a status code, and return `Ok(())` if and only if the value was
    /// `FastlyStatus::OK`. If the status code was some error, then it will be returned in the
    /// result's `Err` variant.
    pub fn result(self) -> Result<(), Self> {
        if let Self::OK = self {
            Ok(())
        } else {
            Err(self)
        }
    }
}

impl fmt::Debug for FastlyStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            Self::OK => "OK",
            Self::ERROR => "ERROR",
            Self::INVAL => "INVAL",
            Self::BADF => "BADF",
            Self::BUFLEN => "BUFLEN",
            Self::UNSUPPORTED => "UNSUPPORTED",
            Self::BADALIGN => "BADALIGN",
            Self::HTTPINVALID => "HTTP_INVALID_ERROR",
            Self::HTTPUSER => "HTTP_USER_ERROR",
            Self::HTTPINCOMPLETE => "HTTP_INCOMPLETE_MESSAGE",
            Self::NONE => "NONE",
            Self::HTTPHEADTOOLARGE => "HTTP_HEAD_TOO_LARGE",
            Self::HTTPINVALIDSTATUS => "HTTP_INVALID_STATUS",
            Self::LIMITEXCEEDED => "LIMIT_EXCEEDED",
            _ => "UNKNOWN",
        })
    }
}

pub const FASTLY_ABI_VERSION: u64 = 1;

// define our own enum rather than using `http`'s, so that we can easily convert it to a scalar
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u32)]
pub enum HttpVersion {
    Http09 = 0,
    Http10 = 1,
    Http11 = 2,
    H2 = 3,
    H3 = 4,
}

impl HttpVersion {
    pub fn as_u32(&self) -> u32 {
        *self as u32
    }
}

// TODO ACF 2019-12-04: could use num-derive for this, but I don't think it's worth pulling in a
// whole new set of dependencies when this will likely be encoded by witx shortly
impl TryFrom<u32> for HttpVersion {
    type Error = String;

    fn try_from(x: u32) -> Result<Self, Self::Error> {
        if x == Self::Http09 as u32 {
            Ok(Self::Http09)
        } else if x == Self::Http10 as u32 {
            Ok(Self::Http10)
        } else if x == Self::Http11 as u32 {
            Ok(Self::Http11)
        } else if x == Self::H2 as u32 {
            Ok(Self::H2)
        } else if x == Self::H3 as u32 {
            Ok(Self::H3)
        } else {
            Err(format!("unknown http version enum value: {}", x))
        }
    }
}

impl From<http::Version> for HttpVersion {
    fn from(v: http::Version) -> Self {
        match v {
            http::Version::HTTP_09 => Self::Http09,
            http::Version::HTTP_10 => Self::Http10,
            http::Version::HTTP_11 => Self::Http11,
            http::Version::HTTP_2 => Self::H2,
            http::Version::HTTP_3 => Self::H3,
            _ => unreachable!(),
        }
    }
}

impl From<HttpVersion> for http::Version {
    fn from(v: HttpVersion) -> Self {
        match v {
            HttpVersion::Http09 => Self::HTTP_09,
            HttpVersion::Http10 => Self::HTTP_10,
            HttpVersion::Http11 => Self::HTTP_11,
            HttpVersion::H2 => Self::HTTP_2,
            HttpVersion::H3 => Self::HTTP_3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u32)]
pub enum BodyWriteEnd {
    Back = 0,
    Front = 1,
}

/// Determines how the framing headers (`Content-Length`/`Transfer-Encoding`) are set for a
/// request or response.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u32)]
pub enum FramingHeadersMode {
    /// Determine the framing headers automatically based on the message body, and discard any framing
    /// headers already set in the message. This is the default behavior.
    ///
    /// In automatic mode, a `Content-Length` is used when the size of the body can be determined
    /// before it is sent. Requests/responses sent in streaming mode, where headers are sent immediately
    /// but the content of the body is streamed later, will receive a `Transfer-Encoding: chunked`
    /// to accommodate the dynamic generation of the body.
    Automatic = 0,

    /// Use the exact framing headers set in the message, falling back to [`Automatic`][`Self::Automatic`]
    /// if invalid.
    ///
    /// In "from headers" mode, any `Content-Length` or `Transfer-Encoding` headers will be honored.
    /// You must ensure that those headers have correct values permitted by the
    /// [HTTP/1.1 specification][spec]. If the provided headers are not permitted by the spec,
    /// the headers will revert to automatic mode and a log diagnostic will be issued about what was
    /// wrong. If a `Content-Length` is permitted by the spec, but the value doesn't match the size of
    /// the actual body, the body will either be truncated (if it is too long), or the connection will
    /// be hung up early (if it is too short).
    ///
    /// [spec]: https://datatracker.ietf.org/doc/html/rfc7230#section-3.3.1
    ManuallyFromHeaders = 1,
}

impl Default for FramingHeadersMode {
    fn default() -> Self {
        Self::Automatic
    }
}

/// Determines whether the client is encouraged to stop using the current connection and to open a
/// new one for the next request.
///
/// Most applications do not need to change this setting.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u32)]
pub enum HttpKeepaliveMode {
    /// This is the default behavor.
    Automatic = 0,

    /// Send `Connection: close` in HTTP/1 and a GOAWAY frame in HTTP/2 and HTTP/3.  This prompts
    /// the client to close the current connection and to open a new one for the next request.
    NoKeepalive = 1,
}

impl Default for HttpKeepaliveMode {
    fn default() -> Self {
        Self::Automatic
    }
}

/// Optional override for response caching behavior.
#[derive(Clone, Debug)]
pub enum CacheOverride {
    /// Do not override the behavior specified in the origin response's cache control headers.
    None,
    /// Do not cache the response to this request, regardless of the origin response's headers.
    Pass,
    /// Override particular cache control settings.
    ///
    /// The origin response's cache control headers will be used for ttl and stale_while_revalidate if `None`.
    Override {
        ttl: Option<u32>,
        stale_while_revalidate: Option<u32>,
        pci: bool,
        surrogate_key: Option<HeaderValue>,
    },
}

impl Default for CacheOverride {
    fn default() -> Self {
        Self::default()
    }
}

impl CacheOverride {
    pub const fn none() -> Self {
        Self::None
    }

    pub const fn pass() -> Self {
        Self::Pass
    }

    pub fn is_pass(&self) -> bool {
        if let Self::Pass = self {
            true
        } else {
            false
        }
    }

    pub const fn ttl(ttl: u32) -> Self {
        Self::Override {
            ttl: Some(ttl),
            stale_while_revalidate: None,
            pci: false,
            surrogate_key: None,
        }
    }

    pub const fn stale_while_revalidate(swr: u32) -> Self {
        Self::Override {
            ttl: None,
            stale_while_revalidate: Some(swr),
            pci: false,
            surrogate_key: None,
        }
    }

    pub const fn pci(pci: bool) -> Self {
        Self::Override {
            ttl: None,
            stale_while_revalidate: None,
            pci,
            surrogate_key: None,
        }
    }

    pub const fn surrogate_key(sk: HeaderValue) -> Self {
        Self::Override {
            ttl: None,
            stale_while_revalidate: None,
            pci: false,
            surrogate_key: Some(sk),
        }
    }

    pub fn set_none(&mut self) {
        *self = Self::None;
    }

    pub fn set_pass(&mut self, pass: bool) {
        if pass {
            *self = Self::Pass;
        } else if let Self::Pass = self {
            *self = Self::None;
        }
    }

    pub fn get_ttl(&self) -> Option<u32> {
        if let Self::Override { ttl, .. } = self {
            *ttl
        } else {
            None
        }
    }

    pub fn set_ttl(&mut self, new_ttl: u32) {
        match self {
            Self::Override { ttl, .. } => *ttl = Some(new_ttl),
            _ => *self = Self::ttl(new_ttl),
        }
    }

    pub fn get_stale_while_revalidate(&self) -> Option<u32> {
        if let Self::Override {
            stale_while_revalidate,
            ..
        } = self
        {
            *stale_while_revalidate
        } else {
            None
        }
    }

    pub fn set_stale_while_revalidate(&mut self, new_swr: u32) {
        match self {
            Self::Override {
                stale_while_revalidate,
                ..
            } => *stale_while_revalidate = Some(new_swr),
            _ => *self = Self::stale_while_revalidate(new_swr),
        }
    }

    pub fn set_pci(&mut self, new_pci: bool) {
        match self {
            Self::Override { pci, .. } => *pci = new_pci,
            _ => *self = Self::pci(new_pci),
        }
    }

    pub fn get_surrogate_key(&self) -> Option<&HeaderValue> {
        if let Self::Override { surrogate_key, .. } = self {
            surrogate_key.as_ref()
        } else {
            None
        }
    }

    pub fn set_surrogate_key(&mut self, new_surrogate_key: HeaderValue) {
        match self {
            Self::Override { surrogate_key, .. } => *surrogate_key = Some(new_surrogate_key),
            _ => *self = Self::surrogate_key(new_surrogate_key),
        }
    }

    pub const fn default() -> Self {
        Self::None
    }

    /// Convert to a representation suitable for passing across the ABI boundary.
    ///
    /// The representation contains the `CacheOverrideTag` along with all of the possible fields:
    /// `(tag, ttl, swr, sk)`.
    #[doc(hidden)]
    pub fn to_abi(&self) -> (u32, u32, u32, Option<&[u8]>) {
        match *self {
            Self::None => (CacheOverrideTag::empty().bits(), 0, 0, None),
            Self::Pass => (CacheOverrideTag::PASS.bits(), 0, 0, None),
            Self::Override {
                ttl,
                stale_while_revalidate,
                pci,
                ref surrogate_key,
            } => {
                let mut tag = CacheOverrideTag::empty();
                let ttl = if let Some(ttl) = ttl {
                    tag |= CacheOverrideTag::TTL;
                    ttl
                } else {
                    0
                };
                let swr = if let Some(swr) = stale_while_revalidate {
                    tag |= CacheOverrideTag::STALE_WHILE_REVALIDATE;
                    swr
                } else {
                    0
                };
                if pci {
                    tag |= CacheOverrideTag::PCI;
                }
                let sk = surrogate_key.as_ref().map(HeaderValue::as_bytes);
                (tag.bits(), ttl, swr, sk)
            }
        }
    }

    /// Convert from the representation suitable for passing across the ABI boundary.
    ///
    /// Returns `None` if the tag is not recognized. Depending on the tag, some of the values may be
    /// ignored.
    #[doc(hidden)]
    pub fn from_abi(
        tag: u32,
        ttl: u32,
        swr: u32,
        surrogate_key: Option<HeaderValue>,
    ) -> Option<Self> {
        CacheOverrideTag::from_bits(tag).map(|tag| {
            if tag.contains(CacheOverrideTag::PASS) {
                return CacheOverride::Pass;
            }
            if tag.is_empty() && surrogate_key.is_none() {
                return CacheOverride::None;
            }
            let ttl = if tag.contains(CacheOverrideTag::TTL) {
                Some(ttl)
            } else {
                None
            };
            let stale_while_revalidate = if tag.contains(CacheOverrideTag::STALE_WHILE_REVALIDATE) {
                Some(swr)
            } else {
                None
            };
            let pci = tag.contains(CacheOverrideTag::PCI);
            CacheOverride::Override {
                ttl,
                stale_while_revalidate,
                pci,
                surrogate_key,
            }
        })
    }
}

bitflags::bitflags! {
    /// A bit field used to tell the host which fields are used when setting the cache override.
    ///
    /// If the `PASS` bit is set, all other bits are ignored.
    struct CacheOverrideTag: u32 {
        const PASS = 1 << 0;
        const TTL = 1 << 1;
        const STALE_WHILE_REVALIDATE = 1 << 2;
        const PCI = 1 << 3;
    }
}

#[derive(Debug)]
pub enum ClientCertVerifyResult {
    /// Success value.
    ///
    /// This indicates that client certificate verified successfully.
    Ok,
    /// Bad certificate error.
    ///
    /// This error means the certificate is corrupt
    /// (e.g., the certificate signatures do not verify correctly).
    BadCertificate,
    /// Certificate revoked error.
    ///
    /// This error means the client certificate is revoked by its signer.
    CertificateRevoked,
    /// Certificate expired error.
    ///
    /// This error means the client certificate has expired or is not currently valid.
    CertificateExpired,
    /// Unknown CA error.
    ///
    /// This error means the valid certificate chain or partial chain was received, but the
    /// certificate was not accepted because the CA certificate could not be located or could not
    /// be matched with a known trust anchor.
    UnknownCa,
    /// Certificate missing error.
    ///
    /// This error means the client did not provide a certificate during the handshake.
    CertificateMissing,
    /// Certificate unknown error.
    ///
    /// This error means the client certificate was received, but some other (unspecified) issue
    /// arose in processing the certificate, rendering it unacceptable.
    CertificateUnknown,
}

impl ClientCertVerifyResult {
    pub fn from_u32(value: u32) -> ClientCertVerifyResult {
        match value {
            0 => ClientCertVerifyResult::Ok,
            1 => ClientCertVerifyResult::BadCertificate,
            2 => ClientCertVerifyResult::CertificateRevoked,
            3 => ClientCertVerifyResult::CertificateExpired,
            4 => ClientCertVerifyResult::UnknownCa,
            5 => ClientCertVerifyResult::CertificateMissing,
            _ => ClientCertVerifyResult::CertificateUnknown,
        }
    }
}
