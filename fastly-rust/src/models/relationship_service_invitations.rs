/*
 * Fastly API
 *
 * Via the Fastly API you can perform any of the operations that are possible within the management console,  including creating services, domains, and backends, configuring rules or uploading your own application code, as well as account operations such as user administration and billing reports. The API is organized into collections of endpoints that allow manipulation of objects related to Fastly services and accounts. For the most accurate and up-to-date API reference content, visit our [Developer Hub](https://developer.fastly.com/reference/api/) 
 *
 */




#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct RelationshipServiceInvitations {
    #[serde(rename = "service_invitations", skip_serializing_if = "Option::is_none")]
    pub service_invitations: Option<Box<crate::models::RelationshipServiceInvitationsServiceInvitations>>,
}

impl RelationshipServiceInvitations {
    pub fn new() -> RelationshipServiceInvitations {
        RelationshipServiceInvitations {
            service_invitations: None,
        }
    }
}

