
If the body does not contain a valid UTF-8 string, this function will panic. To explicitly handle
the possibility of invalid UTF-8 data, use [`into_body_str_lossy()`][`Self::into_body_str_lossy()`]
for lossy conversion, or use [`into_body_bytes()`][`Self::into_body_bytes()`] and then convert the
bytes explicitly with a function like [`String::from_utf8`].

