
If the body does not contain a valid UTF-8 string, this function will panic. To explicitly handle
the possibility of invalid UTF-8 data, use [`into_bytes()`][`Self::into_bytes()`] and then convert
the bytes explicitly with a function like [`String::from_utf8`].

