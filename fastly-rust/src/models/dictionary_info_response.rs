/*
 * Fastly API
 *
 * Via the Fastly API you can perform any of the operations that are possible within the management console,  including creating services, domains, and backends, configuring rules or uploading your own application code, as well as account operations such as user administration and billing reports. The API is organized into collections of endpoints that allow manipulation of objects related to Fastly services and accounts. For the most accurate and up-to-date API reference content, visit our [Developer Hub](https://developer.fastly.com/reference/api/) 
 *
 */




#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct DictionaryInfoResponse {
    /// Timestamp (UTC) when the dictionary was last updated or an item was added or removed.
    #[serde(rename = "last_updated", skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
    /// The number of items currently in the dictionary.
    #[serde(rename = "item_count", skip_serializing_if = "Option::is_none")]
    pub item_count: Option<i32>,
    /// A hash of all the dictionary content.
    #[serde(rename = "digest", skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

impl DictionaryInfoResponse {
    pub fn new() -> DictionaryInfoResponse {
        DictionaryInfoResponse {
            last_updated: None,
            item_count: None,
            digest: None,
        }
    }
}

