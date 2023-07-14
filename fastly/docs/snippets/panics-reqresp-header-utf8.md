
This method panics if the value of the header is not a valid UTF-8 string. To handle the possibility
of invalid UTF-8 data, use [`get_header_str_lossy()`][`Self::get_header_str_lossy()`] for lossy
conversion, or use [`get_header()`][`Self::get_header()`] and then convert the bytes with
[`HeaderValue::to_str()`].

