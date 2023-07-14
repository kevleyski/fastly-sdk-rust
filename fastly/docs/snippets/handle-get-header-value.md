
Get the value of a header, or `None` if the header is not present.

If there are multiple values for the header, only one is returned. See
[`get_header_values()`][`Self::get_header_values()`] if you need to get all of the values.

If the value is longer than `max_len`, this will return a [`BufferSizeError`]; you can retry
with a larger buffer size if necessary.

