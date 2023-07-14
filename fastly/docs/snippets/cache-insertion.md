For the insertion to complete successfully, the object must be written into the [`StreamingBody`],
and then [`StreamingBody::finish`] must be called. If the [`StreamingBody`] is dropped before calling
[`StreamingBody::finish`], the insertion is considered incomplete, and any concurrent lookups that
may be reading from the object as it is streamed into the cache may encounter a streaming error.
