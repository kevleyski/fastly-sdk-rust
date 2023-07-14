Sets the size of the cached item, in bytes, when known prior to actually providing the bytes.

It is preferable to provide a length, if possible. Clients that begin streaming the item's
contents before it is completely provided will see the promised length which allows them to,
for example, use `content-length` instead of `transfer-encoding: chunked` if the item is
used as the body of a [`Request`][`crate::Request`] or [`Response`][`crate::Response`].
