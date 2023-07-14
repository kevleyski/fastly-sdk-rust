
# Memory usage

This method will cause the entire body to be buffered in WebAssembly memory. You should take
care not to exceed the WebAssembly memory limits, and consider using [`Read`] methods to
control how much of the body you process at once.

