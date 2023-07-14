# Incompatibility with [`fastly::main`][`crate::main`]

This method cannot be used with [`fastly::main`][`crate::main`], as that
attribute implicitly calls [`Response::send_to_client()`] on the returned
response. Use an undecorated `main()` function instead, along with
[`Request::from_client()`] if the client request is needed.
