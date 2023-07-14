/*
 * Fastly API
 *
 * Via the Fastly API you can perform any of the operations that are possible within the management console,  including creating services, domains, and backends, configuring rules or uploading your own application code, as well as account operations such as user administration and billing reports. The API is organized into collections of endpoints that allow manipulation of objects related to Fastly services and accounts. For the most accurate and up-to-date API reference content, visit our [Developer Hub](https://developer.fastly.com/reference/api/) 
 *
 */

/// PopCoordinates : the geographic location of the POP



#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct PopCoordinates {
    #[serde(rename = "latitude")]
    pub latitude: f32,
    #[serde(rename = "longitude")]
    pub longitude: f32,
}

impl PopCoordinates {
    /// the geographic location of the POP
    pub fn new(latitude: f32, longitude: f32) -> PopCoordinates {
        PopCoordinates {
            latitude,
            longitude,
        }
    }
}


