Only one stream can be active at a time for a given
[`Found`]. `Err(CacheError::InvalidOperation)` will be returned if a stream is
already active for this [`Found`]. This restriction may be lifted in future
releases.
