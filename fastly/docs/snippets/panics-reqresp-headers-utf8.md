
This method panics if any of the header values are not valid UTF-8 strings. To handle the
possibility of non-UTF-8 data, use
[`get_header_all_str_lossy()`][`Self::get_header_all_str_lossy()`] for lossy conversion, or use
[`get_header_all()`][`Self::get_header_all()`] and then convert the bytes with
[`HeaderValue::to_str()`].

