/// Try evaluating an expression of type `Result<T, E>`, returning `Err(SendError)` with the given
/// backend and request upon failure.
///
/// This is handy because the `?` syntax doesn't work for `Error`s in a function that returns
/// `Result<T, SendError>`, and the usual way we'd convert with `.map_err(|e|
/// SendError::new(backend, req, e))` requires moving `backend` and `req` even when the error
/// closure is never run.
macro_rules! try_with_req {
    ( $backend:expr, $req:expr, $expr:expr ) => {
        match $expr {
            Ok(x) => x,
            Err(e) => return Err(SendError::new($backend, $req, e)),
        }
    };
}

/// Like `try_with_req`, but gets the backend and sent request from a `PendingRequest`.
macro_rules! try_with_pending_req {
    ( $pr:expr, $expr:expr ) => {
        match $expr {
            Ok(x) => x,
            Err(e) => return Err(SendError::from_pending_req($pr, e)),
        }
    };
}
