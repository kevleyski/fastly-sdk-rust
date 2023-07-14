use super::{Backend, MAX_BACKEND_NAME_LEN};
use crate::abi::fastly_http_req::register_dynamic_backend;
use fastly_shared::{FastlyStatus, SslVersion};
use fastly_sys::{BackendConfigOptions, DynamicBackendConfig};
use std::time::Duration;
use thiserror::Error;

/// A builder structure for generating a dynamic backend.
///
/// This structure can be constructed using either
/// [`BackendExt::builder()`][crate::experimental::BackendExt::builder()] or its own
/// [`new()`][Self::new()] method, and will generate a new backend for use by the program after
/// consuming the `BackendBuilder` with [`finish()`][Self::finish()].
pub struct BackendBuilder {
    name: String,
    target: String,
    host_override: Option<String>,
    connect_timeout: Option<Duration>,
    first_byte_timeout: Option<Duration>,
    between_bytes_timeout: Option<Duration>,
    use_ssl: bool,
    min_tls_version: Option<SslVersion>,
    max_tls_version: Option<SslVersion>,
    cert_hostname: Option<String>,
    ca_cert: Option<String>,
    ciphers: Option<String>,
    sni_hostname: Option<String>,
    pool_connections: bool,
}

/// Errors that can arise from attempting to create a dynamic backend.
///
/// Perhaps the most critical of these is `Disallowed`, which will occur
/// if your service is not permitted to use dynamic backends.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum BackendCreationError {
    /// Timeouts for backends must be less than 2^32 milliseconds, or
    /// about a month and a half.
    #[error("Connect timeout too long; must be < 2^32 milliseconds")]
    ConnectTimeoutTooLarge(Duration),
    /// Timeouts for backends must be less than 2^32 milliseconds, or
    /// about a month and a half.
    #[error("First byte timeout too long; must be < 2^32 milliseconds")]
    FirstByteTimeoutTooLarge(Duration),
    /// Timeouts for backends must be less than 2^32 milliseconds, or
    /// about a month and a half.
    #[error("Between-byte timeout too long; must be < 2^32 milliseconds")]
    BetweenBytesTimeoutTooLarge(Duration),
    /// This service is not allowed to create dynamic backends.
    ///
    /// If you'd like to use dynamic backends, please contact your Fastly sales agent.
    #[error("Dynamic backends not supported for this service")]
    Disallowed,
    /// Something internal went wrong with the service at runtime; you
    /// may be able to do something to react to this information.
    ///
    /// This value is identical to the values underlying `FastlyStatus`.
    #[error("Host failed with status {0:?}")]
    HostError(FastlyStatus),
    /// There was a problem converting the new name from the host into
    /// something we could turn into a Rust `String`.
    ///
    /// Please check the prefix you provided, if you provided one, and make sure it's reasonable.
    #[error(transparent)]
    EncodingError(#[from] std::string::FromUtf8Error),
    /// The backend name provided was too long; please keep it to <255
    /// characters.
    #[error("Provided backend name too long: {0}")]
    NameTooLong(String),
    /// The backend name is already in use.
    #[error("The provided backend name is already in use")]
    NameInUse,
}

impl From<FastlyStatus> for BackendCreationError {
    fn from(x: FastlyStatus) -> Self {
        match x {
            FastlyStatus::UNSUPPORTED => BackendCreationError::Disallowed,
            FastlyStatus::ERROR => BackendCreationError::NameInUse,
            _ => BackendCreationError::HostError(x),
        }
    }
}

impl BackendBuilder {
    #[doc = include_str!("../../docs/snippets/dynamic-backend-builder.md")]
    pub fn new(name: impl ToString, target: impl ToString) -> Self {
        BackendBuilder {
            name: name.to_string(),
            target: target.to_string(),
            host_override: None,
            connect_timeout: None,
            first_byte_timeout: None,
            between_bytes_timeout: None,
            // TODO: Should the default actually be to use SSL?
            use_ssl: false,
            min_tls_version: None,
            max_tls_version: None,
            cert_hostname: None,
            ca_cert: None,
            ciphers: None,
            sni_hostname: None,
            pool_connections: true,
        }
    }

    /// Set a host header override when contacting this backend.
    ///
    /// This will force the value of the "Host" header to the given string when sending out the
    /// origin request. If this is not set and no header already exists, the "Host" header will
    /// default to this builder's target.
    ///
    /// For more information, see the Fastly documentation on override hosts here:
    /// <https://docs.fastly.com/en/guides/specifying-an-override-host>
    pub fn override_host(mut self, name: impl ToString) -> Self {
        self.host_override = Some(name.to_string());
        self
    }

    /// Set the connection timeout for this backend. Defaults to 1,000ms (1s).
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Set a timeout that applies between the time of connection and the time
    /// we get the first byte back. Defaults to 15,000ms (15s).
    pub fn first_byte_timeout(mut self, timeout: Duration) -> Self {
        self.first_byte_timeout = Some(timeout);
        self
    }

    /// Set a timeout that applies between any two bytes we receive across the
    /// wire. Defaults to 10,000ms (10s).
    pub fn between_bytes_timeout(mut self, timeout: Duration) -> Self {
        self.between_bytes_timeout = Some(timeout);
        self
    }

    /// Use SSL/TLS to connect to the backend.
    pub fn enable_ssl(mut self) -> Self {
        self.use_ssl = true;
        self
    }

    /// Disable SSL/TLS for this backend.
    pub fn disable_ssl(mut self) -> Self {
        self.use_ssl = false;
        self
    }

    /// Set the minimum TLS version for connecting to the backend. Setting
    /// this will enable SSL for the connection as a side effect.
    pub fn set_min_tls_version(mut self, minimum: SslVersion) -> Self {
        self.use_ssl = true;
        self.min_tls_version = Some(minimum);
        self
    }

    /// Set the maximum TLS version for connecting to the backend. Setting
    /// this will enable SSL for the connection as a side effect.
    pub fn set_max_tls_version(mut self, maximum: SslVersion) -> Self {
        self.use_ssl = true;
        self.max_tls_version = Some(maximum);
        self
    }

    /// Define the hostname that the server certificate should declare, and
    /// turn on validation during backend connections. You should enable this
    /// if you are using SSL/TLS, and setting this will enable SSL for the
    /// connection as a side effect.
    pub fn check_certificate(mut self, hostname: impl ToString) -> Self {
        self.use_ssl = true;
        self.cert_hostname = Some(hostname.to_string());
        self
    }

    /// Set the CA certificate to use when checking the validity of the
    /// backend. Setting this will enable SSL for the connection as a side
    /// effect.
    pub fn ca_certificate(mut self, value: impl ToString) -> Self {
        self.use_ssl = true;
        self.ca_cert = Some(value.to_string());
        self
    }

    /// Set the acceptable cipher suites to use for an SSL connection. Setting
    /// this will enable SSL for the connection as a side effect.
    pub fn tls_ciphers(mut self, value: impl ToString) -> Self {
        self.use_ssl = true;
        self.ciphers = Some(value.to_string());
        self
    }

    /// Set the SNI hostname for the backend connection. Setting this will
    /// enable SSL for the connection as a side effect.
    pub fn sni_hostname(mut self, value: impl ToString) -> Self {
        self.use_ssl = true;
        self.sni_hostname = Some(value.to_string());
        self
    }

    /// Determine whether or not connections to the same backend should be pooled
    /// across different sessions.
    ///
    /// Fastly considers two backends "the same" if they're registered with the
    /// same name and the exact same settings. In those cases, when pooling is
    /// enabled, if Session 1 opens a connection to this backend it will be left
    /// open, and can be re-used by Session 2. This can help improve backend
    /// latency, by removing the need for the initial network / TLS handshake(s).
    ///
    /// By default, pooling is enabled for dynamic backends.
    pub fn enable_pooling(mut self, value: bool) -> Self {
        self.pool_connections = value;
        self
    }

    /// Attempt to register this backend with runtime, returning the backend
    /// for use like any other backends.
    ///
    /// In the case that this function returns `BackendCreationError::NameInUse`,
    /// users can use `Backend::from_str` as per normal to create a reference to
    /// that version. (That being said, you should be careful to only use this
    /// capability in situations in which you are 100% sure that this name will
    /// always lead to the same place.)
    pub fn finish(self) -> Result<Backend, BackendCreationError> {
        let name = self.name.as_ptr();
        let name_len = self.name.len();

        if name_len > MAX_BACKEND_NAME_LEN {
            return Err(BackendCreationError::NameTooLong(self.name));
        }

        let target = self.target.as_ptr();
        let target_len = self.target.len();

        // now that we've got all the required things ready to go, let's build our
        // config structures.
        let mut config_options = BackendConfigOptions::empty();
        let mut config = DynamicBackendConfig::default();

        if let Some(host_override) = self.host_override.as_deref() {
            config.host_override = host_override.as_ptr();
            config.host_override_len = host_override.bytes().count() as u32;
            config_options.insert(BackendConfigOptions::HOST_OVERRIDE);
        }

        if let Some(connect_timeout) = self.connect_timeout {
            config.connect_timeout_ms = connect_timeout
                .as_millis()
                .try_into()
                .map_err(|_| BackendCreationError::ConnectTimeoutTooLarge(connect_timeout))?;
            config_options.insert(BackendConfigOptions::CONNECT_TIMEOUT);
        }

        if let Some(first_byte_timeout) = self.first_byte_timeout {
            config.first_byte_timeout_ms = first_byte_timeout
                .as_millis()
                .try_into()
                .map_err(|_| BackendCreationError::FirstByteTimeoutTooLarge(first_byte_timeout))?;
            config_options.insert(BackendConfigOptions::FIRST_BYTE_TIMEOUT);
        }

        if let Some(between_bytes_timeout) = self.between_bytes_timeout {
            config.between_bytes_timeout_ms =
                between_bytes_timeout.as_millis().try_into().map_err(|_| {
                    BackendCreationError::BetweenBytesTimeoutTooLarge(between_bytes_timeout)
                })?;
            config_options.insert(BackendConfigOptions::BETWEEN_BYTES_TIMEOUT);
        }

        if self.use_ssl {
            config_options.insert(BackendConfigOptions::USE_SSL);
        }

        if let Some(min_tls_version) = self.min_tls_version {
            config.ssl_min_version = min_tls_version as u32;
            config_options.insert(BackendConfigOptions::SSL_MIN_VERSION);
        }

        if let Some(max_tls_version) = self.max_tls_version {
            config.ssl_max_version = max_tls_version as u32;
            config_options.insert(BackendConfigOptions::SSL_MAX_VERSION);
        }

        if let Some(hostname) = self.cert_hostname.as_deref() {
            config.cert_hostname = hostname.as_ptr();
            config.cert_hostname_len = hostname.bytes().count() as u32;
            config_options.insert(BackendConfigOptions::CERT_HOSTNAME);
        }

        if let Some(string) = self.ca_cert.as_deref() {
            config.ca_cert = string.as_ptr();
            config.ca_cert_len = string.bytes().count() as u32;
            config_options.insert(BackendConfigOptions::CA_CERT);
        }

        if let Some(string) = self.ciphers.as_deref() {
            config.ciphers = string.as_ptr();
            config.ciphers_len = string.bytes().count() as u32;
            config_options.insert(BackendConfigOptions::CIPHERS);
        }

        if let Some(string) = self.sni_hostname.as_deref() {
            config.sni_hostname = string.as_ptr();
            config.sni_hostname_len = string.bytes().count() as u32;
            config_options.insert(BackendConfigOptions::SNI_HOSTNAME);
        }

        if !self.pool_connections {
            config_options.insert(BackendConfigOptions::DONT_POOL);
        }

        let basic_result = unsafe {
            register_dynamic_backend(name, name_len, target, target_len, config_options, &config)
        };

        match basic_result {
            FastlyStatus::OK => Ok(Backend { name: self.name }),
            _ => Err(BackendCreationError::from(basic_result)),
        }
    }
}
