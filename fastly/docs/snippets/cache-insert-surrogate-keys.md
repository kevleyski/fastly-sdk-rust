Sets the surrogate keys that can be used for purging this cached item.

[Surrogate key purges][`crate::http::purge::purge_surrogate_key`] are the only
means to purge specific items from the cache. At least one surrogate key must be
set in order to remove an item without performing a
[purge-all](https://developer.fastly.com/learning/concepts/purging/#purge-all),
waiting for the item's TTL to elapse, or overwriting the item with [`insert()`].

Surrogate keys must contain only printable ASCII characters (those between 0x21
and 0x7E, inclusive). Any invalid keys will be ignored.

See the [Fastly surrogate keys
guide](https://docs.fastly.com/en/guides/purging-api-cache-with-surrogate-keys)
for details.
