
# Memory usage

This method will cause the entire body to be buffering in WebAssembly memory. You should take care
not to exceed the WebAssembly memory limits, and consider using methods like
[`read_body_lines()`][`Self::read_body_lines()`] or
[`read_body_chunks()`][`Self::read_body_chunks()`] to control how much of the body you process at
once.

