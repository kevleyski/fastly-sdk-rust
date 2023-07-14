//! Backend server.
mod builder;

use crate::abi::{self, FastlyStatus};
pub use builder::*;
use fastly_shared::SslVersion;
use http::HeaderValue;
use std::{str::FromStr, time::Duration};

/// The maximum length in characters of a backend name.
pub(crate) const MAX_BACKEND_NAME_LEN: usize = 255;

/// A named backend.
///
/// This represents a backend associated with a service that we can send requests to, potentially
/// caching the responses received.
///
/// Backends come in one of two flavors:
///   * **Static Backends**: These backends are created using the Fastly UI or API,
///     and are predefined by the user. Static backends have short names (see the
///     precise naming rules in [`Backend::from_name`]) that are usable across every
///     session of a service.
///   * **Dynamic Backends**: These backends are created programmatically using the
///     [`Backend::builder`] API. They are defined at runtime, and may or may not be
///     shared across sessions depending on how they are configured.
///
/// To use a backend, pass it to the [`crate::Request::send`] method. Alternatively, the following
/// values can be automatically coerced into a backend for you, without the need to explicitly
/// create a `Backend` object, although they may all induce panics:
///
///   * Any string type ([`&str`][`str`], [`String`, or `&String`][`String`]) will
///     be automatically turned into the static backend of the same name.
///
/// ## Using Static Backends
///
/// As stated at the top level, the following snippet is a minimal program that resends
/// a request to a static backend named "example_backend":
///
/// ```no_run
/// use fastly::{Error, Request, Response};
///
/// #[fastly::main]
/// fn main(ds_req: Request) -> Result<Response, Error> {
///     Ok(ds_req.send("example_backend")?)
/// }
/// ```
///
/// A safer alternative to this example would be the following:
///
/// ```no_run
/// use fastly::{Backend, Error, Request, Response};
///
/// #[fastly::main]
/// fn main(ds_req: Request) -> Result<Response, Error> {
///     match Backend::from_name("example_backend") {
///        Ok(backend) => Ok(ds_req.send(backend)?),
///        Err(_) => {
///             // custom backend failure response
///             unimplemented!()
///        }
///     }
/// }
/// ```
///
/// as this version allows you to handle backend errors more cleanly.
///
/// ## Validating Support for Dynamic Backends
///
/// Since dynamic backends are only enabled for some services, it may be particularly
/// useful to ensure that dynamic backends are supported in your Compute@Edge service.
/// The easiest way to do so is to try to create a dynamic backend, and then explicitly
/// check for the [`crate::backend::BackendCreationError::Disallowed`] code, as follows:
///
/// ```no_run
/// use fastly::{Backend, Error, Request, Response};
/// use fastly::backend::BackendCreationError;
///
/// #[fastly::main]
/// fn main(ds_req: Request) -> Result<Response, Error> {
///     match Backend::builder("custom_backend", "example.org:993").finish() {
///         Ok(backend) => Ok(ds_req.send(backend)?),
///         Err(BackendCreationError::Disallowed) => {
///            // custom code ofr handling when dynamic backends aren't supported
///            unimplemented!()
///         }
///         Err(err) => {
///            // more specific logging/handling for backend misconfigurations
///            unimplemented!()
///         }
///     }
/// }
/// ```
///
///
///
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Backend {
    name: String,
}

impl Backend {
    /// Get a backend by its name.
    ///
    /// This function will return a [`BackendError`] if an invalid name was given.
    ///
    /// Backend names:
    ///   * cannot be empty
    ///   * cannot be longer than 255 characters
    ///   * cannot ASCII control characters such as `'\n'` or `DELETE`.
    ///   * cannot contain special Unicode characters
    ///   * should only contain visible ASCII characters or spaces
    ///
    /// Future versions of this function may return an error if your service does not have a backend
    /// with this name.
    pub fn from_name(s: &str) -> Result<Self, BackendError> {
        s.parse()
    }

    #[doc = include_str!("../docs/snippets/dynamic-backend-builder.md")]
    pub fn builder(name: impl ToString, target: impl ToString) -> BackendBuilder {
        BackendBuilder::new(name.to_string(), target.to_string())
    }

    /// Get the name of this backend.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Turn the backend into its name as a string.
    pub fn into_string(self) -> String {
        self.name
    }

    /// Returns true if a backend with this name exists.
    pub fn exists(&self) -> bool {
        let mut exists = 0;
        unsafe { abi::fastly_backend::exists(self.name.as_ptr(), self.name.len(), &mut exists) }
            .result()
            .map(|_| exists == 1)
            .expect("fastly_backend::exists failed")
    }

    /// Returns true if this is a dynamic backend.
    ///
    /// # Panics
    ///
    #[doc = include_str!("../docs/snippets/panics-backend-must-exist.md")]
    pub fn is_dynamic(&self) -> bool {
        let mut is = 0;
        unsafe { abi::fastly_backend::is_dynamic(self.name.as_ptr(), self.name.len(), &mut is) }
            .result()
            .map(|_| is == 1)
            .expect("fastly_backend::is_dynamic failed")
    }

    /// Returns the hostname for this backend.
    ///
    /// # Panics
    ///
    #[doc = include_str!("../docs/snippets/panics-backend-must-exist.md")]
    pub fn get_host(&self) -> String {
        // First, ask the host how long our buffer will need to be. Provide a null pointer, with
        // a maximum length of 0. We should get back a `BUFLEN` code, with `nwritten` set to the
        // length of the host.
        let mut nwritten = 0;
        let length = match unsafe {
            abi::fastly_backend::get_host(
                self.name.as_ptr(),
                self.name.len(),
                std::ptr::null_mut(),
                0,
                &mut nwritten,
            )
        } {
            FastlyStatus::BUFLEN => nwritten,
            status => panic!(
                "fastly_backend::get_host returned an unexpected result: {:?}",
                status
            ),
        };

        // Now, call once more with a sufficiently long buffer.
        let mut buf = Vec::with_capacity(length);
        unsafe {
            abi::fastly_backend::get_host(
                self.name.as_ptr(),
                self.name.len(),
                buf.as_mut_ptr(),
                buf.capacity(),
                &mut nwritten,
            )
        }
        .result()
        .expect("fastly_backend::get_host returned an unexpected result");

        assert!(
            nwritten <= buf.capacity(),
            "fastly_backend::get_host wrote too many bytes"
        );
        unsafe {
            // Safety:
            // - We assert above that `nwritten` is less than or equal to `capacity`.
            // - We assume that the host did write to `old_len..new_len`.
            buf.set_len(nwritten);
        }

        String::from_utf8(buf).expect("fastly_backend::get_host returns valid UTF-8 bytes")
    }

    /// Returns the host header override when contacting this backend.
    ///
    /// This method returns `None` if no host header override is configured for this backend.
    ///
    /// This is used to change the `Host` header sent to the backend. For more information, see
    /// the Fastly documentation on host overrides here:
    /// <https://docs.fastly.com/en/guides/specifying-an-override-host>
    ///
    /// Use
    /// [`BackendBuilder::override_host`][self::builder::BackendBuilder::override_host]
    /// to set this for a dynamic backend.
    ///
    /// # Panics
    ///
    #[doc = include_str!("../docs/snippets/panics-backend-must-exist.md")]
    pub fn get_host_override(&self) -> Option<HeaderValue> {
        // First, ask the host how long our buffer will need to be. Provide a null pointer, with
        // a maximum length of 0. We should get back a `BUFLEN` code, with `nwritten` set to the
        // length of the host.
        let mut nwritten = 0;
        let length = match unsafe {
            abi::fastly_backend::get_override_host(
                self.name.as_ptr(),
                self.name.len(),
                std::ptr::null_mut(),
                0,
                &mut nwritten,
            )
        } {
            // If a `NONE` code is returned, there is no host override. We are finished!
            FastlyStatus::NONE => return None,
            FastlyStatus::BUFLEN => nwritten,
            status => panic!(
                "fastly_backend::get_override_host returned an unexpected result: {:?}",
                status
            ),
        };

        // Now, call once more with a sufficiently long buffer.
        let mut buf = Vec::with_capacity(length);
        unsafe {
            abi::fastly_backend::get_override_host(
                self.name.as_ptr(),
                self.name.len(),
                buf.as_mut_ptr(),
                buf.capacity(),
                &mut nwritten,
            )
        }
        .result()
        .expect("fastly_backend::get_override_host returned an unexpected result");

        assert!(
            nwritten <= buf.capacity(),
            "fastly_backend::get_override_host wrote too many bytes"
        );
        unsafe {
            // Safety:
            // - We assert above that `nwritten` is less than or equal to `capacity`.
            // - We assume that the host did write to `old_len..new_len`.
            buf.set_len(nwritten);
        }

        Some(
            HeaderValue::try_from(buf)
                .expect("fastly_backend::get_override_host returns valid header value bytes"),
        )
    }

    /// Get the port number of the backend's address.
    ///
    /// # Panics
    ///
    #[doc = include_str!("../docs/snippets/panics-backend-must-exist.md")]
    pub fn get_port(&self) -> u16 {
        let mut port = 0;
        unsafe { abi::fastly_backend::get_port(self.name.as_ptr(), self.name.len(), &mut port) }
            .result()
            .map(|_| port)
            .expect("fastly_backend::get_port returned an unexpected result")
    }

    /// Returns the connection timeout for this backend.
    ///
    /// Use
    /// [`BackendBuilder::connect_timeout`][self::builder::BackendBuilder::connect_timeout]
    /// to set this for a dynamic backend.
    ///
    /// # Panics
    ///
    #[doc = include_str!("../docs/snippets/panics-backend-must-exist.md")]
    pub fn get_connect_timeout(&self) -> Duration {
        let mut timeout = 0;
        unsafe {
            abi::fastly_backend::get_connect_timeout_ms(
                self.name.as_ptr(),
                self.name.len(),
                &mut timeout,
            )
        }
        .result()
        .map(|_| timeout)
        .map(u64::from)
        .map(Duration::from_millis)
        .expect("fastly_backend::get_connect_timeout_ms returned an unexpected result")
    }

    /// Returns the "first byte" timeout for this backend.
    ///
    /// This timeout applies between the time of connection and the time we get the first byte back.
    ///
    /// Use
    /// [`BackendBuilder::first_byte_timeout`][self::builder::BackendBuilder::first_byte_timeout]
    /// to set this for a dynamic backend.
    ///
    /// # Panics
    ///
    #[doc = include_str!("../docs/snippets/panics-backend-must-exist.md")]
    pub fn get_first_byte_timeout(&self) -> Duration {
        let mut timeout = 0;
        unsafe {
            abi::fastly_backend::get_first_byte_timeout_ms(
                self.name.as_ptr(),
                self.name.len(),
                &mut timeout,
            )
        }
        .result()
        .map(|_| timeout)
        .map(u64::from)
        .map(Duration::from_millis)
        .expect("fastly_backend::get_first_byte_timeout returned an unexpected result")
    }

    /// Returns the "between bytes" timeout for this backend.
    ///
    /// This timeout applies between any two bytes we receive across the wire.
    ///
    /// Use
    /// [`BackendBuilder::between_bytes_timeout`][self::builder::BackendBuilder::between_bytes_timeout]
    /// to set this for a dynamic backend.
    ///
    /// # Panics
    ///
    #[doc = include_str!("../docs/snippets/panics-backend-must-exist.md")]
    pub fn get_between_bytes_timeout(&self) -> Duration {
        let mut timeout = 0;
        unsafe {
            abi::fastly_backend::get_between_bytes_timeout_ms(
                self.name.as_ptr(),
                self.name.len(),
                &mut timeout,
            )
        }
        .result()
        .map(|_| timeout)
        .map(u64::from)
        .map(Duration::from_millis)
        .expect("fastly_backend::get_between_bytes_timeout returned an unexpected result")
    }

    /// Returns `true` if SSL/TLS is used to connect to the backend.
    ///
    /// Use
    /// [`BackendBuilder::enable_ssl`][self::builder::BackendBuilder::enable_ssl] or
    /// [`BackendBuilder::disable_ssl`][self::builder::BackendBuilder::disable_ssl]
    /// to set this for a dynamic backend.
    ///
    /// # Panics
    ///
    #[doc = include_str!("../docs/snippets/panics-backend-must-exist.md")]
    pub fn is_ssl(&self) -> bool {
        let mut is = 0;
        unsafe { abi::fastly_backend::is_ssl(self.name.as_ptr(), self.name.len(), &mut is) }
            .result()
            .map(|_| is == 1)
            .expect("fastly_backend::is_ssl returned an unexpected result")
    }

    /// Returns the minimum TLS version for connecting to the backend.
    ///
    /// This method returns `None` if SSL/TLS is not enabled for this backend.
    ///
    /// Use
    /// [`BackendBuilder::set_min_tls_version`][self::builder::BackendBuilder::set_min_tls_version]
    /// to set this for a dynamic backend.
    ///
    /// # Panics
    ///
    #[doc = include_str!("../docs/snippets/panics-backend-must-exist.md")]
    pub fn get_ssl_min_version(&self) -> Option<SslVersion> {
        let mut min = 0;
        match unsafe {
            abi::fastly_backend::get_ssl_min_version(self.name.as_ptr(), self.name.len(), &mut min)
        }
        .result()
        {
            Ok(_) => Some(min.try_into().unwrap()),
            Err(FastlyStatus::NONE) => None,
            other => panic!(
                "fastly_backend::get_ssl_min_version returned an unexpected result: {other:?}"
            ),
        }
    }

    /// Returns the maximum TLS version for connecting to the backend.
    ///
    /// This method returns `None` if SSL/TLS is not enabled for this backend.
    ///
    /// Use
    /// [`BackendBuilder::set_max_tls_version`][self::builder::BackendBuilder::set_max_tls_version]
    /// to set this for a dynamic backend.
    ///
    /// # Panics
    ///
    #[doc = include_str!("../docs/snippets/panics-backend-must-exist.md")]
    pub fn get_ssl_max_version(&self) -> Option<SslVersion> {
        let mut max = 0;
        match unsafe {
            abi::fastly_backend::get_ssl_max_version(self.name.as_ptr(), self.name.len(), &mut max)
        }
        .result()
        {
            Ok(_) => Some(max.try_into().unwrap()),
            Err(FastlyStatus::NONE) => None,
            other => panic!(
                "fastly_backend::get_ssl_max_version returned an unexpected result: {other:?}"
            ),
        }
    }
}

/// [`Backend`]-related errors.
#[derive(Copy, Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum BackendError {
    /// The backend name was empty.
    #[error("an empty string is not a valid backend")]
    EmptyName,
    /// The backend name was too long.
    #[error("backend names must be <= 255 characters")]
    TooLong,
    /// The backend name contained invalid characters.
    #[error("backend names must only contain visible ASCII characters or spaces")]
    InvalidName,
}

impl FromStr for Backend {
    type Err = BackendError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        validate_backend(s)?;
        Ok(Self { name: s.to_owned() })
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name.as_str())
    }
}

/// Validate that a string looks like an acceptable [`Backend`] value.
///
/// Note that this is *not* meant to be a filter for things that could cause security issues, it is
/// only meant to catch errors before the hostcalls do in order to yield friendlier error messages.
///
/// This function will return a [`BackendError`] if an invalid name was given.
///
/// Backend names:
///   * cannot be empty
///   * cannot be longer than 255 characters
///   * cannot ASCII control characters such as `'\n'` or `DELETE`.
///   * cannot contain special Unicode characters
///   * should only contain visible ASCII characters or spaces
//
// TODO KTM 2020-03-10: We should not allow VCL keywords like `if`, `now`, `true`, `urldecode`, and
// so on. Once we have better errors, let's make sure that these are caught in a pleasant manner.
pub fn validate_backend(backend: &str) -> Result<(), BackendError> {
    if backend.is_empty() {
        Err(BackendError::EmptyName)
    } else if backend.len() > MAX_BACKEND_NAME_LEN {
        Err(BackendError::TooLong)
    } else if backend.chars().any(is_invalid_char) {
        Err(BackendError::InvalidName)
    } else {
        Ok(())
    }
}

/// Return true if a character is not allowed in a [`Backend`] name.
///
/// This is used to enforce the rules described in the documentation of [`Backend::from_name()`]. A
/// backend name should only contain visible ASCII characters, or spaces.
#[inline]
fn is_invalid_char(c: char) -> bool {
    c != ' ' && !c.is_ascii_graphic()
}

#[cfg(test)]
mod validate_backend_tests {
    use super::*;

    #[test]
    fn valid_backend_names_are_accepted() {
        let valid_backend_names = [
            "valid_backend_1",
            "1_backend_with_leading_integer",
            "backend-with-kebab-case",
            "backend with spaces",
            "backend.with.periods",
            "123_456_789_000",
            "123.456.789.000",
            "tilde~backend",
            "(parens-backend)",
        ];
        for backend in valid_backend_names.iter() {
            match validate_backend(backend) {
                Ok(_) => {}
                x => panic!(
                    "backend string \"{}\" yielded unexpected result: {:?}",
                    backend, x
                ),
            }
        }
    }

    #[test]
    fn empty_str_is_not_accepted() {
        let invalid_backend = "";
        match validate_backend(invalid_backend) {
            Err(BackendError::EmptyName) => {}
            x => panic!("unexpected result: {:?}", x),
        }
    }

    #[test]
    fn name_equal_to_character_limit_is_accepted() {
        use std::iter::FromIterator;
        let invalid_backend: String = String::from_iter(vec!['a'; 255]);
        match validate_backend(&invalid_backend) {
            Ok(_) => {}
            x => panic!("unexpected result: {:?}", x),
        }
    }

    #[test]
    fn name_longer_than_character_limit_are_not_accepted() {
        use std::iter::FromIterator;
        let invalid_backend: String = String::from_iter(vec!['a'; 256]);
        match validate_backend(&invalid_backend) {
            Err(BackendError::TooLong) => {}
            x => panic!("unexpected result: {:?}", x),
        }
    }

    #[test]
    fn unprintable_characters_are_not_accepted() {
        let invalid_backend = "\n";
        match validate_backend(invalid_backend) {
            Err(BackendError::InvalidName) => {}
            x => panic!("unexpected result: {:?}", x),
        }
    }

    #[test]
    fn unicode_is_not_accepted() {
        let invalid_backend = "♓";
        match validate_backend(invalid_backend) {
            Err(BackendError::InvalidName) => {}
            x => panic!("unexpected result: {:?}", x),
        }
    }
}
