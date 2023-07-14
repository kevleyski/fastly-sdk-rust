
# Memory usage

This method will cause the entire body to be buffering in WebAssembly memory. You should take care
not to exceed the WebAssembly memory limits, and consider using methods like [`BufRead::lines()`] or
[`Body::read_chunks()`] to control how much of the body you process at once.

