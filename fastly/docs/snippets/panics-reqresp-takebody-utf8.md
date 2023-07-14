
If the body does not contain a valid UTF-8 string, this function will panic. To handle the
possibility of invalid UTF-8 data, use [`take_body_str_lossy()`][`Self::take_body_str_lossy()`] for
lossy conversion, or use [`take_body_bytes()`][`Self::take_body_bytes()`] and then convert the bytes
with a function like [`String::from_utf8`].

