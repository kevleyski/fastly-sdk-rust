**Note**: These headers are narrowly useful for implementing cache lookups
incorporating the semantics of the [HTTP `Vary`
header](https://www.rfc-editor.org/rfc/rfc9110#section-12.5.5), but the APIs in
this module are not suitable for HTTP caching out-of-the-box. Future SDK
releases will contain an HTTP Cache API.

The headers act as additional factors in object selection, and the choice of _which_
headers to factor in is determined during insertion, via e.g. [`InsertBuilder::vary_by`].
A lookup will succeed when there is at least one cached item that matches lookup's cache key,
and all of the lookup's headers included in the cache items' `vary_by` list match the
corresponding headers in that cached item.

A [typical example](https://www.fastly.com/blog/best-practices-using-vary-header)
is a cached HTTP response, where the request had an `Accept-Encoding` header. In that case,
the origin server may or may not decide on a given encoding, and whether that same response is
suitable for a request with a different (or missing) `Accept-Encoding` header is determined
by whether `Accept-Encoding` is listed in `Vary` header in the origin's response.
