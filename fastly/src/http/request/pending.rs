pub use fastly_shared::CacheOverride;

use super::SendError;
use crate::http::response::{handles_to_response, FastlyResponseMetadata};
use crate::{Request, Response};
use std::collections::HashMap;

pub mod handle;
pub use handle::{select_handles, PendingRequestHandle, PollHandleResult};

/// A handle to a pending asynchronous request returned by [`Request::send_async()`] or
/// [`Request::send_async_streaming()`].
///
/// A handle can be evaluated using [`PendingRequest::poll()`], [`PendingRequest::wait()`], or
/// [`select`]. It can also be discarded if the request was sent for effects it might have, and the
/// response is unimportant.
pub struct PendingRequest {
    /// The handle to the pending asynchronous request.
    handle: PendingRequestHandle,
    /// Metadata that will be attached to the [`Response`] once the handle is finished.
    pub(super) metadata: FastlyResponseMetadata,
}

impl PendingRequest {
    /// Create a new pending request.
    ///
    /// Note that this constructor is *not* exposed in the public interface. Users should never
    /// directly invoke this constructor, and will receive a pending request by calling
    /// [`Request::send_async()`] or [`Request::send_async_streaming()`].
    pub(super) fn new(handle: PendingRequestHandle, metadata: FastlyResponseMetadata) -> Self {
        Self { handle, metadata }
    }

    /// Try to get the result of a pending request without blocking.
    ///
    /// This function returns immediately with a [`PollResult`]; if you want to block until a result
    /// is ready, use [`PendingRequest::wait()`].
    ///
    /// # Panics
    ///
    #[doc = include_str!("../../../docs/snippets/panics-responselimits.md")]
    pub fn poll(self) -> PollResult {
        let Self { handle, metadata } = self;
        match handle.copy().poll() {
            PollHandleResult::Pending(handle) => PollResult::Pending(Self { handle, metadata }),
            PollHandleResult::Done(Ok((resp_handle, resp_body_handle))) => {
                PollResult::Done(handles_to_response(resp_handle, resp_body_handle, metadata))
            }
            PollHandleResult::Done(Err(e)) => PollResult::Done(Err(SendError::from_pending_req(
                Self { handle, metadata },
                e,
            ))),
        }
    }

    /// Block until the result of a pending request is ready.
    ///
    /// If you want check whether the result is ready without blocking, use
    /// [`PendingRequest::poll()`].
    ///
    /// # Panics
    ///
    #[doc = include_str!("../../../docs/snippets/panics-responselimits.md")]
    pub fn wait(self) -> Result<Response, SendError> {
        let (resp_handle, resp_body_handle) =
            try_with_pending_req!(self, self.handle.copy().wait());
        handles_to_response(resp_handle, resp_body_handle, self.metadata)
    }

    /// Get a reference to the original [`Request`] associated with this pending request.
    ///
    /// Note that the request's original body is already sending, so the returned request does not
    /// have a body.
    pub fn sent_req(&self) -> &Request {
        self.metadata
            .sent_req()
            .expect("sent_req must be present for a pending request")
    }
}

/// The result of a call to [`PendingRequest::poll()`].
pub enum PollResult {
    /// The request is still in progress, and can be polled again.
    Pending(PendingRequest),
    /// The request has either completed or errored.
    Done(Result<Response, SendError>),
}

/// Given a collection of [`PendingRequest`]s, block until the result of one of the requests is
/// ready.
///
/// This function accepts any type which can become an iterator that yields requests; a common
/// choice is `Vec<PendingRequest>`.
///
/// Returns a tuple `(result, remaining)`, where:
///
/// - `result` is the result of the request that became ready.
///
/// - `remaining` is a vector containing all of the requests that did not become ready. The order of
/// the requests in this vector is not guaranteed to match the order of the requests in the argument
/// collection.
///
/// ### Examples
///
/// **Selecting using the request URI**
///
/// You can use [`Response::get_backend_request()`] to inspect the request that a response came
/// from. This example uses the URL to see which of the two requests finished first:
///
/// ```no_run
/// use fastly::{Error, Request};
/// # fn f() -> Result<(), Error> { // Wrap the example in a function, so we can propagate errors.
///
/// // Send two asynchronous requests, and store the pending requests in a vector.
/// let req1 = Request::get("http://www.origin.org/meow")
///     .send_async("TheOrigin")?;
/// let req2 = Request::get("http://www.origin.org/woof")
///     .send_async("TheOrigin")?;
/// let pending_reqs = vec![req1, req2];
///
/// // Wait for one of the requests to finish.
/// let (resp, _remaining) = fastly::http::request::select(pending_reqs);
///
/// // Return an error if the request was not successful.
/// let resp = resp?;
///
/// // Inspect the response metadata to see which backend this response came from.
/// match resp
///     .get_backend_request()
///     .unwrap()
///     .get_url()
///     .path()
/// {
///     "/meow" => println!("I love cats!"),
///     "/woof" => println!("I love dogs!"),
///     _ => panic!("unexpected result"),
/// }
///
/// # Ok(())
/// # }
/// ```
///
/// **Selecting using the backend name**
///
/// You can also use [`Response::get_backend_name()`] to identify which pending request in the given
/// collection finished. Consider this example, where two requests are sent asynchronously to two
/// different backends:
///
/// ```no_run
/// use fastly::{Error, Request};
/// # fn f() -> Result<(), Error> { // Wrap the example in a function, so we can propagate errors.
///
/// // Send two asynchronous requests, and store the pending requests in a vector.
/// let req1 = Request::get("http://www.origin-1.org/")
///     .send_async("origin1")?;
/// let req2 = Request::get("http://www.origin-2.org/")
///     .send_async("origin2")?;
/// let pending_reqs = vec![req1, req2];
///
/// // Wait for one of the requests to finish.
/// let (resp, _remaining) = fastly::http::request::select(pending_reqs);
///
/// // Return an error if the request was not successful.
/// let resp = resp?;
///
/// // Inspect the response to see which backend this response came from.
/// match resp.get_backend_name().unwrap() {
///     "origin1" => println!("origin 1 responded first!"),
///     "origin2" => println!("origin 2 responded first!"),
///     _ => panic!("unexpected result"),
/// }
///
/// # Ok(())
/// # }
/// ```
///
/// ### Panics
///
/// Panics if the argument collection is empty, or contains more than
/// [`fastly_shared::MAX_PENDING_REQS`] requests.
///
#[doc = include_str!("../../../docs/snippets/panics-responselimits.md")]
pub fn select<I>(pending_reqs: I) -> (Result<Response, SendError>, Vec<PendingRequest>)
where
    I: IntoIterator<Item = PendingRequest>,
{
    // Before we call the underlying handles API with `select_handles`, we need to do some
    // book-keeping. `PendingRequest` has some additional members we need to preserve across calls.
    // So first, we split our `PendingRequest` iterator into two collections:
    //     (1) a vector of pending request handles
    //     (2) a map of (handle, sent_req) key-value pairs
    let (handles, mut handles_metadata) = {
        let pending_reqs = pending_reqs.into_iter().collect::<Vec<_>>();
        let mut handles = Vec::with_capacity(pending_reqs.len());
        let mut handles_metadata = HashMap::with_capacity(pending_reqs.len());
        for PendingRequest { handle, metadata } in pending_reqs {
            handles_metadata.insert(handle.as_u32(), metadata);
            handles.push(handle);
        }
        (handles, handles_metadata)
    }; // Next, block until one of the handles is ready.
    let (res, _, remaining_handles) = select_handles(handles);
    let remaining = {
        // Now that a request finished, we need to stitch the remaining pending request handles
        // back together with their corresponding `sent_req` values, before we handle the response.
        let mut remaining = Vec::with_capacity(remaining_handles.len());
        for handle in remaining_handles {
            let metadata = handles_metadata
                .remove(&handle.as_u32())
                .expect("handle exists in sent_req map");
            remaining.push(PendingRequest { handle, metadata });
        }
        remaining
    };
    // Finally, we need to take the final entry from our metadata map, which belongs to the request
    // that finished. Use this, together with the response handles, to build our `Response<Body>`.
    assert_eq!(handles_metadata.len(), 1); // There should only be one (1) entry left.
    let (_, mut metadata) = handles_metadata.into_iter().next().unwrap();
    let res = match res {
        Ok((resp_handle, resp_body_handle)) => {
            handles_to_response(resp_handle, resp_body_handle, metadata)
        }
        Err(e) => {
            let sent_req = metadata
                .take_sent_req()
                .expect("sent_req must be present for a pending request");
            Err(SendError::new(
                metadata
                    .backend()
                    .expect("backend must be present for a pending request")
                    .name(),
                sent_req,
                e,
            ))
        }
    };
    // We're all done! Return the response and the remaining pending requests.
    (res, remaining)
}
