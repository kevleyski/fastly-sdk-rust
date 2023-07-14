
# Panics

This conversion trait differs from [`std::convert::Into`], which is guaranteed to succeed for any
argument, and [`std::convert::TryInto`] which returns an explicit error when a conversion fails.

For types marked above as **Can panic?**, the conversion may panic at runtime if the data is
invalid. Automatic conversions for these types are provided for convenience for data you trust, like
a string literal in your code, or for applications where a default `500 Internal Server Error`
response for a conversion failure is acceptable.

In most applications you should explicitly convert data from untrusted sources, such as the client
request, to one of the types that cannot fail at runtime using a method like those listed under
**Non-panicking conversion**. This allows your application to handle conversion errors without
panicking, such as by falling back on a default value, removing invalid characters, or returning a
branded error page.

