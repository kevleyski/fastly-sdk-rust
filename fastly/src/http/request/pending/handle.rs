pub use fastly_shared::CacheOverride;

use super::PendingRequest;
use crate::abi;
use crate::error::Error;
use crate::handle::{BodyHandle, ResponseHandle};
use crate::http::request::SendErrorCause;

/// A handle to a pending asynchronous request returned by
/// [`RequestHandle::send_async()`][`crate::handle::RequestHandle::send_async()`] or
/// [`RequestHandle::send_async_streaming()`][`crate::handle::RequestHandle::send_async_streaming()`].
///
/// A handle can be evaluated using [`PendingRequestHandle::poll()`],
/// [`PendingRequestHandle::wait()`], or [`select_handles()`][`crate::handle::select_handles()`]. It
/// can also be discarded if the request was sent for effects it might have, and the response is
/// unimportant.
#[derive(Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct PendingRequestHandle {
    handle: u32,
}

impl From<PendingRequest> for PendingRequestHandle {
    fn from(pr: PendingRequest) -> Self {
        pr.handle
    }
}

impl PendingRequestHandle {
    pub(crate) const INVALID: Self = Self {
        handle: fastly_shared::INVALID_PENDING_REQUEST_HANDLE,
    };

    pub(crate) fn is_invalid(&self) -> bool {
        self == &Self::INVALID
    }

    /// Make a handle from its underlying representation.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub(crate) fn from_u32(handle: u32) -> Self {
        Self { handle }
    }

    /// Get the underlying representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub fn as_u32(&self) -> u32 {
        self.handle
    }

    /// Make a copy of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub(crate) fn copy(&self) -> Self {
        Self {
            handle: self.handle,
        }
    }

    /// Get a mutable reference to the underlying `u32` representation of the handle.
    ///
    /// This should only be used when calling the raw ABI directly, and care should be taken not to
    /// reuse or alias handle values.
    pub(crate) fn as_u32_mut(&mut self) -> &mut u32 {
        &mut self.handle
    }

    /// Try to get the result of a pending request without blocking.
    ///
    /// This function returns immediately with a [`PollHandleResult`]; if you want to block until a
    /// result is ready, use [`wait()`][`Self::wait()`].
    pub fn poll(self) -> PollHandleResult {
        let mut is_done = -1;
        let mut resp_handle = ResponseHandle::INVALID;
        let mut body_handle = BodyHandle::INVALID;
        let status = unsafe {
            abi::fastly_http_req::pending_req_poll(
                self.as_u32(),
                &mut is_done,
                resp_handle.as_u32_mut(),
                body_handle.as_u32_mut(),
            )
        };

        // An error indicates either that a handle was invalid or that a request did get polled and
        // had some error for us. For example, we could begin receiving a response that gets
        // truncated early; we might poll it ready with an Incomplete status.
        if status.is_err() {
            return PollHandleResult::Done(Err(SendErrorCause::status(status)));
        }

        if is_done < 0 || is_done > 1 {
            // For witx reasons, is_done is indicated by a 0 or 1, rather than a "boolean" type.
            // Getting an out of range value here should be impossible.
            panic!("fastly_http_req_pending_req_poll internal error");
        }
        let is_done = if is_done == 0 { false } else { true };
        if !is_done {
            return PollHandleResult::Pending(self);
        }
        if is_done && (resp_handle.is_invalid() || body_handle.is_invalid()) {
            PollHandleResult::Done(Err(SendErrorCause::Generic(Error::msg(
                "asynchronous request failed",
            ))))
        } else {
            PollHandleResult::Done(Ok((resp_handle, body_handle)))
        }
    }

    /// Block until the result of a pending request is ready.
    ///
    /// If you want check whether the result is ready without blocking, use
    /// [`poll()`][`Self::poll()`].
    pub fn wait(self) -> Result<(ResponseHandle, BodyHandle), SendErrorCause> {
        let mut resp_handle = ResponseHandle::INVALID;
        let mut body_handle = BodyHandle::INVALID;
        let status = unsafe {
            abi::fastly_http_req::pending_req_wait(
                self.as_u32(),
                resp_handle.as_u32_mut(),
                body_handle.as_u32_mut(),
            )
        };
        if status.is_err() {
            return Err(SendErrorCause::status(status));
        }

        if resp_handle.is_invalid() || body_handle.is_invalid() {
            panic!("fastly_http_req::pending_req_wait returned invalid handles");
        }
        Ok((resp_handle, body_handle))
    }
}

/// The result of a call to [`PendingRequestHandle::poll()`].
pub enum PollHandleResult {
    /// The request is still in progress, and can be polled again using the given handle.
    Pending(PendingRequestHandle),
    /// The request has either completed or errored.
    Done(Result<(ResponseHandle, BodyHandle), SendErrorCause>),
}

/// Given a collection of [`PendingRequestHandle`]s, block until the result of one of the handles is
/// ready.
///
/// This function accepts any type which can become an iterator that yields handles; a common choice
/// is `Vec<PendingRequestHandle>`.
///
/// Returns a tuple `(result, index, remaining)`, where:
///
/// - `result` is the result of the handle that became ready.
///
/// - `index` is the index of the handle in the argument collection (e.g., the index of the handle
/// in a vector) that became ready.
///
/// - `remaining` is a vector containing all of the handles that did not become ready. The order of
/// the handles in this vector is not guaranteed to match the order of the handles in the argument
/// collection.
///
/// ### Panics
///
/// Panics if the argument collection is empty, or contains more than
/// [`fastly_shared::MAX_PENDING_REQS`] handles.
pub fn select_handles<I>(
    pending_reqs: I,
) -> (
    Result<(ResponseHandle, BodyHandle), SendErrorCause>,
    usize,
    Vec<PendingRequestHandle>,
)
where
    I: IntoIterator<Item = PendingRequestHandle>,
{
    let mut prs = pending_reqs
        .into_iter()
        .map(|pr| pr.as_u32())
        .collect::<Vec<u32>>();
    if prs.is_empty() || prs.len() > fastly_shared::MAX_PENDING_REQS as usize {
        panic!(
            "the number of selected handles must be at least 1, and less than {}",
            fastly_shared::MAX_PENDING_REQS
        );
    }
    let mut done_index = -1;
    let mut resp_handle = ResponseHandle::INVALID;
    let mut body_handle = BodyHandle::INVALID;

    let status = unsafe {
        abi::fastly_http_req::pending_req_select(
            prs.as_ptr(),
            prs.len(),
            &mut done_index,
            resp_handle.as_u32_mut(),
            body_handle.as_u32_mut(),
        )
    };

    if status.is_err() || done_index < 0 {
        // since we are providing the out-pointers, and an owned `PendingRequestHandle` in Wasm can
        // only exist if it's present in the host, any error returns from the hostcall would
        // indicate an internal (host) bug. Alternatively, the provided set of handles is empty or
        // beyond MAX_PENDING_REQS, and we want to panic here anyway.
        panic!("fastly_http_req_pending_req_select internal error");
    }

    // If we successfully (status is not an error) waited for a handle, we must have gotten
    // something for a handle we provided. This means `done_index` must be valid.
    let done_index = done_index
        .try_into()
        .expect("fastly_http_req_pending_req_select returned an invalid index");

    // quickly remove the completed handle from the set to return
    prs.swap_remove(done_index);

    let res = if resp_handle.is_invalid() || body_handle.is_invalid() {
        // HACK: work around an ABI limitation: for the time being we can't return a FastlyStatus
        // error *and* provide out-parameters. We lose the specific error cause, but we still know
        // from the invalid handle that `done_index` is the failed request.
        Err(SendErrorCause::Generic(Error::msg(
            "selected request failed",
        )))
    } else {
        Ok((resp_handle, body_handle))
    };

    (
        res,
        done_index,
        prs.into_iter()
            .map(PendingRequestHandle::from_u32)
            .collect(),
    )
}
