//! Geographic data for IP addresses.

pub use time::UtcOffset;

use crate::abi::{self, FastlyStatus};
use crate::error::BufferSizeError;
use crate::limits;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Look up the geographic data associated with a particular IP address.
///
/// Returns `None` if no geographic data is available, such as when the IP address is reserved for
/// private use.
///
/// # Examples
///
/// To get geographic information for the downstream client:
///
/// ```no_run
/// let client_ip = fastly::Request::from_client().get_client_ip_addr().unwrap();
/// let geo = fastly::geo::geo_lookup(client_ip).unwrap();
/// if let fastly::geo::ConnType::Satellite = geo.conn_type() {
///     println!("receiving a request from outer space 🛸");
/// }
/// ```
pub fn geo_lookup(ip: IpAddr) -> Option<Geo> {
    geo_lookup_raw(ip).map(Geo::from_raw)
}

/// Look up the raw geographic data associated with a particular IP address.
///
/// Returns `None` if no geographic data is available, such as when the IP address is reserved for
/// private use. The returned `RawGeo` may contain fields with Fastly-documented error values,
/// which the higher-level `Geo` will translate to `Option` or `Result`.
fn geo_lookup_raw(ip: IpAddr) -> Option<RawGeo> {
    use std::net::IpAddr::{V4, V6};
    let (addr_bytes, addr_len) = match ip {
        V4(ip) => (ip.octets().to_vec(), 4),
        V6(ip) => (ip.octets().to_vec(), 16),
    };

    let result = match geo_lookup_impl(&addr_bytes, addr_len, limits::INITIAL_GEO_BUF_SIZE) {
        Ok(g) => g,
        Err(BufferSizeError {
            needed_buf_size, ..
        }) => geo_lookup_impl(&addr_bytes, addr_len, needed_buf_size).ok()?,
    };

    // Try to parse any non-null response, returning `None` otherwise.
    result.and_then(|geo_bytes| serde_json::from_slice::<'_, RawGeo>(&geo_bytes).ok())
}

pub(crate) fn geo_lookup_impl(
    addr_bytes: &[u8],
    addr_len: usize,
    max_length: usize,
) -> Result<Option<Vec<u8>>, BufferSizeError> {
    let mut buf = Vec::with_capacity(max_length);
    let mut nwritten: usize = 0;
    let status = unsafe {
        abi::fastly_geo::lookup(
            addr_bytes.as_ptr(),
            addr_len,
            buf.as_mut_ptr(),
            buf.capacity(),
            &mut nwritten,
        )
    };
    match status.result() {
        Ok(_) => {
            assert!(
                nwritten <= buf.capacity(),
                "fastly_geo::lookup wrote too many bytes"
            );
            unsafe {
                buf.set_len(nwritten);
            }
            Ok(Some(buf))
        }
        Err(FastlyStatus::BUFLEN) => Err(BufferSizeError::geo(max_length, nwritten)),
        Err(_) => Ok(None),
    }
}

/// The raw geographic data associated with a particular IP address.
///
/// Users of the `fastly` crate likely want the [`geo::Geo`] struct instead, from
/// [`geo::geo_lookup`], which will handle some error modes possible in the raw result.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct RawGeo {
    as_name: String,
    as_number: u32,
    area_code: u16,
    city: String,
    #[serde(deserialize_with = "deserialize_conn_speed")]
    conn_speed: ConnSpeed,
    #[serde(deserialize_with = "deserialize_conn_type")]
    conn_type: ConnType,
    #[serde(deserialize_with = "deserialize_continent")]
    continent: Continent,
    country_code: String,
    country_code3: String,
    country_name: String,
    latitude: f64,
    longitude: f64,
    metro_code: i64,
    postal_code: String,
    #[serde(deserialize_with = "deserialize_proxy_description")]
    proxy_description: ProxyDescription,
    #[serde(deserialize_with = "deserialize_proxy_type")]
    proxy_type: ProxyType,
    region: Option<String>,
    utc_offset: i32,
}

/// The geographic data associated with a particular IP address.
// TODO ACF 2020-04-20: make a nicer type for the AS fields once the IANA licensing question is
// sorted out. https://www.iana.org/assignments/as-numbers/as-numbers.xhtml
//
// TODO ACF 2020-04-20: we should be able to represent the continent, country, region, etc much more
// nicely than this, however the licensing for ISO data appears to be fraught. The `locale-codes`
// crate looks very nice, but it sources its data from a CC BY-SA 4.0 repo despite being
// MIT-licensed, which in turn scrapes wikipedia and other sources. For now, just use strings.
#[derive(Clone, Debug)]
pub struct Geo {
    as_name: String,
    as_number: u32,
    area_code: u16,
    city: String,
    conn_speed: ConnSpeed,
    conn_type: ConnType,
    continent: Continent,
    country_code: String,
    country_code3: String,
    country_name: String,
    latitude: f64,
    longitude: f64,
    metro_code: i64,
    postal_code: String,
    proxy_description: ProxyDescription,
    proxy_type: ProxyType,
    region: Option<String>,
    utc_offset: Option<UtcOffset>,
}

impl Geo {
    fn from_raw(raw: RawGeo) -> Self {
        let utc_offset = if raw.utc_offset != 9999 {
            let hours = (raw.utc_offset as u16 / 100) as i8;
            let minutes = (raw.utc_offset as u16 % 100) as i8;
            UtcOffset::from_hms(hours, minutes, 0).ok()
        } else {
            None
        };
        Geo {
            as_name: raw.as_name,
            as_number: raw.as_number,
            area_code: raw.area_code,
            city: raw.city,
            conn_speed: raw.conn_speed,
            conn_type: raw.conn_type,
            continent: raw.continent,
            country_code: raw.country_code,
            country_code3: raw.country_code3,
            country_name: raw.country_name,
            latitude: raw.latitude,
            longitude: raw.longitude,
            metro_code: raw.metro_code,
            postal_code: raw.postal_code,
            proxy_description: raw.proxy_description,
            proxy_type: raw.proxy_type,
            region: raw.region,
            utc_offset,
        }
    }

    /// The name of the organization associated with `as_number`.
    ///
    /// For example, `fastly` is the value given for IP addresses under AS-54113.
    pub fn as_name(&self) -> &str {
        self.as_name.as_str()
    }

    /// [Autonomous system](https://en.wikipedia.org/wiki/Autonomous_system_(Internet)) (AS) number.
    pub fn as_number(&self) -> u32 {
        self.as_number
    }

    /// The telephone area code associated with an IP address.
    ///
    /// These are only available for IP addresses in the United States, its territories, and Canada.
    pub fn area_code(&self) -> u16 {
        self.area_code
    }

    /// City or town name.
    pub fn city(&self) -> &str {
        self.city.as_str()
    }

    /// Connection speed.
    pub fn conn_speed(&self) -> ConnSpeed {
        self.conn_speed.clone()
    }

    /// Connection type.
    pub fn conn_type(&self) -> ConnType {
        self.conn_type.clone()
    }

    /// Continent.
    pub fn continent(&self) -> Continent {
        self.continent.clone()
    }

    /// A two-character [ISO 3166-1][iso] country code for the country associated with an IP address.
    ///
    /// The US country code is returned for IP addresses associated with overseas United States military bases.
    ///
    /// These values include subdivisions that are assigned their own country codes in ISO
    /// 3166-1. For example, subdivisions NO-21 and NO-22 are presented with the country code SJ for
    /// Svalbard and the Jan Mayen Islands.
    ///
    /// [iso]: https://en.wikipedia.org/wiki/ISO_3166-1
    pub fn country_code(&self) -> &str {
        self.country_code.as_str()
    }

    /// A three-character [ISO 3166-1 alpha-3][iso] country code for the country associated with the IP address.
    ///
    /// The USA country code is returned for IP addresses associated with overseas United States
    /// military bases.
    ///
    /// [iso]: https://en.wikipedia.org/wiki/ISO_3166-1_alpha-3
    pub fn country_code3(&self) -> &str {
        self.country_code3.as_str()
    }

    /// Country name.
    ///
    /// This field is the [ISO 3166-1][iso] English short name for a country.
    ///
    /// [iso]: https://en.wikipedia.org/wiki/ISO_3166-1
    pub fn country_name(&self) -> &str {
        self.country_name.as_str()
    }

    /// Latitude, in units of degrees from the equator.
    ///
    /// Values range from -90.0 to +90.0 inclusive, and are based on the [WGS 84][wgs84] coordinate
    /// reference system.
    ///
    /// [wgs84]: https://en.wikipedia.org/wiki/World_Geodetic_System
    pub fn latitude(&self) -> f64 {
        self.latitude
    }

    /// Longitude, in units of degrees from the [IERS Reference Meridian][iers].
    ///
    /// Values range from -180.0 to +180.0 inclusive, and are based on the [WGS 84][wgs84]
    /// coordinate reference system.
    ///
    /// [iers]: https://en.wikipedia.org/wiki/IERS_Reference_Meridian
    /// [wgs84]: https://en.wikipedia.org/wiki/World_Geodetic_System
    pub fn longitude(&self) -> f64 {
        self.longitude
    }

    /// Metro code, representing designated market areas (DMAs) in the United States.
    pub fn metro_code(&self) -> i64 {
        self.metro_code
    }

    /// The postal code associated with the IP address.
    ///
    /// These are available for some IP addresses in Australia, Canada, France, Germany, Italy,
    /// Spain, Switzerland, the United Kingdom, and the United States.
    ///
    /// For Canadian postal codes, this is the first 3 characters. For the United Kingdom, this is
    /// the first 2-4 characters (outward code). For countries with alphanumeric postal codes, this
    /// field is a lowercase transliteration.
    pub fn postal_code(&self) -> &str {
        self.postal_code.as_str()
    }

    /// Client proxy description.
    pub fn proxy_description(&self) -> ProxyDescription {
        self.proxy_description.clone()
    }

    /// Client proxy type.
    pub fn proxy_type(&self) -> ProxyType {
        self.proxy_type.clone()
    }

    /// [ISO 3166-2][iso] country subdivision code.
    ///
    /// For countries with multiple levels of subdivision (for example, nations within the United
    /// Kingdom), this variable gives the more specific subdivision.
    ///
    /// This field can be `None` for countries that do not have ISO country subdivision codes. For
    /// example, `None` is given for IP addresses assigned to the Åland Islands (country code AX,
    /// illustrated below).
    ///
    /// # Examples
    ///
    /// Region values are the subdivision part only. For typical use, a subdivision is normally
    /// formatted with its associated country code. The following example illustrates constructing
    /// an [ISO 3166-2][iso] two-part country and subdivision code from the respective fields:
    ///
    /// ```no_run
    /// # let client_ip = fastly::Request::from_client().get_client_ip_addr().unwrap();
    /// # let geo = fastly::geo::geo_lookup(client_ip).unwrap();
    /// let code = if let Some(region) = geo.region() {
    ///     format!("{}-{}", geo.country_code(), region);
    /// } else {
    ///     format!("{}", geo.country_code());
    /// };
    /// ```
    ///
    /// | `code`     | Region Name       | Country            | ISO 3166-2 subdivision |
    /// | ---------- | ----------------- | ------------------ | ---------------------- |
    /// | `AX`       | Ödkarby           | Åland Islands      | (none)                 |
    /// | `DE-BE`    | Berlin	         | Germany            | Land (State)           |
    /// | `GB-BNH`   | Brighton and Hove | United Kingdom     | Unitary authority      |
    /// | `JP-13`    | 東京都 (Tōkyō-to)  | Japan              | Prefecture             |
    /// | `RU-MOW`   | Москва́ (Moscow)   | Russian Federation | Federal city           |
    /// | `SE-AB`    | Stockholms län    | Sweden             | Län (County)           |
    /// | `US-CA`    | California        | United States      | State                  |
    ///
    /// [iso]: https://en.wikipedia.org/wiki/ISO_3166-2
    pub fn region(&self) -> Option<&str> {
        self.region.as_ref().map(|s| s.as_str())
    }

    /// Time zone offset from coordinated universal time (UTC) for `city`.
    ///
    /// This is represented using the [`UtcOffset`] type from the [`time`] library. See
    /// `time`'s documentation for more details on how to format this type or use it in time
    /// calculations.
    ///
    /// Returns `None` if the geolocation database does not have a time zone offset for this IP
    /// address.
    pub fn utc_offset(&self) -> Option<UtcOffset> {
        self.utc_offset
    }
}

/// Connection speed.
///
/// These connection speeds imply different latencies, as well as throughput.
///
/// See [OC rates][oc] and [T-carrier][t] for background on OC- and T- connections.
///
/// [oc]: https://en.wikipedia.org/wiki/Optical_Carrier_transmission_rates
/// [t]: https://en.wikipedia.org/wiki/T-carrier
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ConnSpeed {
    Broadband,
    Cable,
    Dialup,
    Mobile,
    Oc12,
    Oc3,
    Satellite,
    T1,
    T3,
    #[serde(rename = "ultrabb")]
    UltraBroadband,
    Wireless,
    Xdsl,
    /// A network connection speed that is known, but not in the above list of variants.
    ///
    /// This typically indicates that the geolocation database contains a connection speed
    /// that did not exist when this crate was published.
    Other(String),
}

/// Connection type.
///
/// Defaults to `Unknown` when the connection type is not known.
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ConnType {
    Wired,
    Wifi,
    Mobile,
    Dialup,
    Satellite,
    #[serde(rename = "?")]
    Unknown,
    /// A type of network connection that is known, but not in the above list of variants.
    ///
    /// This typically indicates that the geolocation database contains a connection type
    /// that did not exist when this crate was published.
    Other(String),
}

/// Continent.
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Continent {
    #[serde(rename = "AF")]
    Africa,
    #[serde(rename = "AN")]
    Antarctica,
    #[serde(rename = "AS")]
    Asia,
    #[serde(rename = "EU")]
    Europe,
    #[serde(rename = "NA")]
    NorthAmerica,
    #[serde(rename = "OC")]
    Oceania,
    #[serde(rename = "SA")]
    SouthAmerica,
    /// A continent that is known, but not one of the above variants.
    ///
    /// The Earth is not prone to spontaneously developing new continents, however *names* of
    /// continents might change. If the short name for a continent changes, this is how an unknown
    /// name would be reported.
    Other(String),
}

impl Continent {
    /// Get the two-letter continent code.
    ///
    /// | Continent     | Code |
    /// | ------------- | ---- |
    /// | Africa        | `AF` |
    /// | Asia          | `AS` |
    /// | Europe        | `EU` |
    /// | North America | `NA` |
    /// | South America | `SA` |
    /// | Oceania       | `OC` |
    /// | Antarctica    | `AN` |
    ///
    /// In the case of an unrecognized continent code in the geolocation database, `as_code` may
    /// return `??`.
    pub fn as_code(&self) -> &'static str {
        match self {
            Self::Africa => "AF",
            Self::Antarctica => "AN",
            Self::Asia => "AS",
            Self::Europe => "EU",
            Self::NorthAmerica => "NA",
            Self::Oceania => "OC",
            Self::SouthAmerica => "SA",
            Self::Other(_) => "??",
        }
    }
}

/// Client proxy description.
///
/// Defaults to `Unknown` when an IP address is not known to be a proxy or VPN.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ProxyDescription {
    /// Enables ubiquitous network access to a shared pool of configurable computing resources.
    Cloud,
    /// A host accessing the internet via a web security and data protection cloud provider.
    ///
    /// Example providers with this type of service are Zscaler, Scansafe, and Onavo.
    CloudSecurity,
    /// A proxy used by overriding the client's DNS value for an endpoint host to that of the proxy
    /// instead of the actual DNS value.
    Dns,
    /// The gateway nodes where encrypted or anonymous Tor traffic hits the internet.
    TorExit,
    /// Receives traffic on the Tor network and passes it along; also referred to as "routers".
    TorRelay,
    /// Virtual private network that encrypts and routes all traffic through the VPN server,
    /// including programs and applications.
    Vpn,
    /// Connectivity that is taking place through mobile device web browser software that proxies
    /// the user through a centralized location.
    ///
    /// Examples of such browsers are Opera mobile browsers and UCBrowser.
    WebBrowser,
    /// An IP address that is not known to be a proxy or VPN.
    #[serde(rename = "?")]
    Unknown,
    /// Description of a proxy or VPN that is known, but not in the above list of variants.
    ///
    /// This typically indicates that the geolocation database contains a proxy description that
    /// did not exist when this crate was published.
    Other(String),
}

/// Client proxy type.
///
/// Defaults to `Unknown` when an IP address is not known to be a proxy or VPN.
// TODO ACF 2020-04-22: the docs on https://docs.fastly.com/vcl/variables/client-geo-proxy-type/
// look like they need a refresher, so I did not transcribe them for the individual variants.
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ProxyType {
    Anonymous,
    Aol,
    Blackberry,
    Corporate,
    Edu,
    Hosting,
    Public,
    Transparent,
    #[serde(rename = "?")]
    Unknown,
    /// A type of proxy or VPN that is known, but not in the above list of variants.
    ///
    /// This typically indicates that the geolocation database contains a proxy type that did not
    /// exist when this crate was published.
    Other(String),
}

use serde::Deserializer;

fn deserialize_conn_speed<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<ConnSpeed, D::Error> {
    deserialize_with_unknown(deserializer, ConnSpeed::Other)
}

fn deserialize_conn_type<'de, D: Deserializer<'de>>(deserializer: D) -> Result<ConnType, D::Error> {
    deserialize_with_unknown(deserializer, ConnType::Other)
}

fn deserialize_continent<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Continent, D::Error> {
    deserialize_with_unknown(deserializer, Continent::Other)
}

fn deserialize_proxy_description<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<ProxyDescription, D::Error> {
    deserialize_with_unknown(deserializer, ProxyDescription::Other)
}

fn deserialize_proxy_type<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<ProxyType, D::Error> {
    deserialize_with_unknown(deserializer, ProxyType::Other)
}

fn deserialize_with_unknown<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
    catchall: fn(String) -> T,
) -> Result<T, D::Error> {
    let s: String = String::deserialize(deserializer)?;
    use serde::de::value::StringDeserializer;
    use serde::de::IntoDeserializer;
    let deserializer: StringDeserializer<D::Error> = s.clone().into_deserializer();
    Ok(T::deserialize(deserializer).unwrap_or_else(|_| catchall(s)))
}

#[test]
fn deserialize_partial_geo_responses() {
    let invalid_utc_offset = br#"
    {
        "as_name": "test as",
        "as_number": 11111,
        "area_code": 0,
        "city": "?",
        "conn_speed": "broadband",
        "conn_type": "wired",
        "continent": "OS",
        "country_code": "EA",
        "country_code3": "EAR",
        "country_name": "the entire earth",
        "latitude": 6.5,
        "longitude": -28.8,
        "metro_code": 0,
        "postal_code": "?",
        "proxy_description": "?",
        "proxy_type": "hosting",
        "region": "?",
        "utc_offset": 9999
    }
    "#;
    let deserialized = serde_json::from_slice::<'_, RawGeo>(invalid_utc_offset)
        .ok()
        .map(Geo::from_raw);
    assert!(deserialized.is_some());
    assert!(deserialized.unwrap().utc_offset.is_none());

    let invalid_variant = br#"
    {
        "as_name": "test as",
        "as_number": 11111,
        "area_code": 0,
        "city": "?",
        "conn_speed": "super_broadband",
        "conn_type": "wired",
        "continent": "OS",
        "country_code": "EA",
        "country_code3": "EAR",
        "country_name": "the entire earth",
        "latitude": 6.5,
        "longitude": -28.8,
        "metro_code": 0,
        "postal_code": "?",
        "proxy_description": "?",
        "proxy_type": "hosting",
        "region": "?",
        "utc_offset": 200
    }
    "#;
    let deserialized = serde_json::from_slice::<'_, RawGeo>(invalid_variant)
        .ok()
        .map(Geo::from_raw);
    assert!(deserialized.is_some());
    assert_eq!(
        deserialized.unwrap().conn_speed(),
        ConnSpeed::Other("super_broadband".to_string())
    );
}
