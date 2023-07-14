//! Low-level interface to Fastly's [Real-Time Log Streaming][about] endpoints.
//!
//! Most applications should use the high-level interface provided by
//! [`log-fastly`](https://docs.rs/log-fastly), which includes management of log levels and easier
//! formatting.
//!
//! To write to an [`Endpoint`], you can use any interface that works with [`std::io::Write`],
//! including [`write!()`] and [`writeln!()`].
//!
//! Each write to the endpoint emits a single log line, so any newlines that are present in the
//! message are escaped to the character sequence `"\n"`.
//!
//! [about]: https://docs.fastly.com/en/guides/about-fastlys-realtime-log-streaming-features
use crate::abi;
use fastly_shared::FastlyStatus;
use std::io::Write;
use thiserror::Error;

/// A Fastly logging endpoint.
///
/// Most applications should use the high-level interface provided by
/// [`log-fastly`](https://docs.rs/log-fastly) rather than writing to this interface directly.
///
/// To write to this endpoint, use the [`std::io::Write`] interface. For example:
///
/// ```no_run
/// # use fastly::log::Endpoint;
/// use std::io::Write;
/// let mut endpoint = Endpoint::from_name("my_endpoint");
/// writeln!(endpoint, "Hello from the edge!").unwrap();
/// ```
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct Endpoint {
    handle: u32,
    name: String,
}

// use a custom debug formatter to avoid the noise from the handle
impl std::fmt::Debug for Endpoint {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.debug_struct("Endpoint")
            .field("name", &self.name)
            .finish()
    }
}

/// Logging-related errors.
#[derive(Copy, Clone, Debug, Error, PartialEq, Eq)]
pub enum LogError {
    /// The endpoint could not be found, or is a reserved name.
    #[error("endpoint not found, or is reserved")]
    InvalidEndpoint,
    /// The endpoint name is malformed.
    #[error("malformed endpoint name")]
    MalformedEndpointName,
    /// The endpoint name is too large.
    #[error("endpoint name is too large")]
    NameTooLarge,
}

impl TryFrom<&str> for Endpoint {
    type Error = LogError;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        Self::try_from_name(name)
    }
}

impl TryFrom<String> for Endpoint {
    type Error = LogError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::try_from_name(&name)
    }
}

impl std::io::Write for Endpoint {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut nwritten = 0;
        let status = unsafe {
            abi::fastly_log::write(self.handle(), buf.as_ptr(), buf.len(), &mut nwritten)
        };
        match status {
            FastlyStatus::OK => Ok(nwritten),
            FastlyStatus::BADF => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "fastly_log::write failed: invalid log endpoint handle",
            )),
            FastlyStatus::BUFLEN => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "fastly_log::write failed: log line too long",
            )),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("fastly_log::write failed: {:?}", status),
            )),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Endpoint {
    pub(crate) unsafe fn handle(&self) -> u32 {
        self.handle
    }

    /// Get the name of an `Endpoint`.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Get an `Endpoint` by name.
    ///
    /// # Panics
    ///
    /// If the endpoint name is not valid, this function will panic.
    pub fn from_name(name: &str) -> Self {
        Self::try_from_name(name).unwrap()
    }

    /// Try to get an `Endpoint` by name.
    ///
    /// Currently, the conditions on an endpoint name are:
    ///
    /// - It must not be empty
    ///
    /// - It must not contain newlines (`\n`) or colons (`:`)
    ///
    /// - It must not be `stdout` or `stderr`, which are reserved for debugging.
    pub fn try_from_name(name: &str) -> Result<Self, LogError> {
        validate_endpoint_name(name)?;
        let mut handle = 0u32;
        let status =
            unsafe { abi::fastly_log::endpoint_get(name.as_ptr(), name.len(), &mut handle) };
        match status {
            FastlyStatus::OK => Ok(Endpoint {
                handle,
                name: name.to_owned(),
            }),
            FastlyStatus::INVAL => Err(LogError::InvalidEndpoint),
            FastlyStatus::LIMITEXCEEDED => Err(LogError::NameTooLarge),
            _ => panic!("fastly_log::endpoint_get failed"),
        }
    }
}

fn validate_endpoint_name(name: &str) -> Result<(), LogError> {
    if name.is_empty() || name.find(|c| c == '\n' || c == ':').is_some() {
        Err(LogError::MalformedEndpointName)
    } else {
        Ok(())
    }
}

/// Set the logging endpoint where the message from Rust panics will be written.
///
/// By default, panic output is written to the `stderr` endpoint. Calling this function will
/// override that default with the endpoint, which may be provided as a string or an
/// [`Endpoint`].
///
/// ```no_run
/// fastly::log::set_panic_endpoint("my_error_endpoint").unwrap();
/// panic!("oh no!");
/// // will log "panicked at 'oh no', your/file.rs:line:col" to "my_error_endpoint"
/// ```
pub fn set_panic_endpoint<E>(endpoint: E) -> Result<(), LogError>
where
    E: TryInto<Endpoint, Error = LogError>,
{
    let endpoint = endpoint.try_into()?;
    std::panic::set_hook(Box::new(move |info| {
        // explicitly buffer this with `to_string()` to avoid multiple `write` calls
        write!(endpoint.clone(), "{}", info.to_string()).expect("write succeeds in panic hook");
    }));
    Ok(())
}
