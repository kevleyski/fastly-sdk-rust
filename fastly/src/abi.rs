use bytes::{Buf, Bytes, BytesMut};
pub use fastly_shared::{FastlyStatus, HttpVersion, FASTLY_ABI_VERSION};
pub use fastly_sys::*;

pub(crate) struct MultiValueHostcall<F> {
    fill_buf: F,
    term: u8,
    buf: BytesMut,
    buf_size: usize,
    max_buf_size: Option<usize>,
    cursor: u32,
    is_done: bool,
}

impl<F> MultiValueHostcall<F> {
    pub(crate) fn new(
        term: u8,
        mut initial_buf_size: usize,
        max_buf_size: Option<usize>,
        fill_buf: F,
    ) -> Self {
        if let Some(max) = max_buf_size {
            initial_buf_size = std::cmp::min(initial_buf_size, max);
        }
        Self {
            fill_buf,
            term,
            buf: BytesMut::with_capacity(initial_buf_size),
            buf_size: initial_buf_size,
            max_buf_size,
            cursor: 0,
            is_done: false,
        }
    }
}

/// Errors related to a [`MultiValueHostcall`].
///
/// Users do not directly interact with this error enum. It is most commonly used to propagate an
/// error to the user informing them that the buffer provided to a multi-value hostcall was not
/// sufficient.
///
/// See [`RequestHandle::get_header_names()`][crate::handle::RequestHandle::get_header_names()`] for
/// an example of a hostcall that handles this error.
#[derive(Copy, Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum MultiValueHostcallError {
    /// The provided buffer size was too small.
    ///
    /// This error variant can be used to inform a user that they can try a multi-value hostcall
    /// again, using a larger buffer.
    #[error("MultiValueHostcall buffer too small")]
    BufferTooSmall { needed_buf_size: usize },
    /// The [`FastlyStatus`] error code returned by the closure `fill_buf`.
    #[error("MultiValueHostcall closure returned error: {0:?}")]
    ClosureError(FastlyStatus),
}

impl<F> std::iter::Iterator for MultiValueHostcall<F>
where
    F: Fn(*mut u8, usize, u32, *mut i64, *mut usize) -> FastlyStatus,
{
    type Item = Result<Bytes, MultiValueHostcallError>;

    fn next(&mut self) -> Option<Self::Item> {
        // first fill the buffer, if it's empty
        if self.buf.is_empty() {
            if self.is_done {
                // if there are no more calls to make, and the buffer is empty, we're done
                return None;
            }
            self.buf.reserve(self.buf_size);
            let mut ending_cursor = 0;
            let mut nwritten = 0;
            let status = (self.fill_buf)(
                self.buf.as_mut_ptr(),
                self.buf.capacity(),
                self.cursor,
                &mut ending_cursor,
                &mut nwritten,
            );
            if status.is_err() {
                match status {
                    FastlyStatus::BUFLEN => {
                        let buffer_can_grow = if let Some(max) = self.max_buf_size {
                            // If there is a max buffer size but the requested size is below it, we
                            // can grow and try again.
                            nwritten < max
                        } else {
                            // Otherwise there is no max, so we can always try again.
                            true
                        };
                        if buffer_can_grow && nwritten != 0 {
                            // If we haven't exceeded the max, and we got back a non-zero nwritten,
                            // try the call again with the necessary buffer size.
                            self.buf_size = nwritten;
                            self.buf.reserve(self.buf_size);
                            let status = (self.fill_buf)(
                                self.buf.as_mut_ptr(),
                                self.buf.capacity(),
                                self.cursor,
                                &mut ending_cursor,
                                &mut nwritten,
                            );
                            if status.is_err() {
                                // If we still error out, set done and call it a closure error; it
                                // shouldn't ever be a buffer length error
                                assert!(
                                    !matches!(status, FastlyStatus::BUFLEN),
                                    "adaptive buffer hostcall requested wrong size"
                                );
                                self.is_done = true;
                                return Some(Err(MultiValueHostcallError::ClosureError(status)));
                            }
                        } else {
                            // If we have a buffer length error but growing would exceed the max, we
                            // are done.
                            self.is_done = true;
                            return Some(Err(MultiValueHostcallError::BufferTooSmall {
                                needed_buf_size: nwritten,
                            }));
                        }
                    }
                    status => {
                        self.is_done = true;
                        return Some(Err(MultiValueHostcallError::ClosureError(status)));
                    }
                }
            }
            if nwritten == 0 {
                // if we get no bytes, we're definitely done; this only comes up if there are no
                // values at all, otherwise we see the ending cursor at -1 and stop
                self.is_done = true;
                return None;
            }
            assert!(
                nwritten <= self.buf.capacity(),
                "fill_buf set invalid nwritten: {}, capacity: {}",
                nwritten,
                self.buf.capacity()
            );
            unsafe {
                self.buf.set_len(nwritten);
            }
            if ending_cursor < 0 {
                // no more calls necessary after this one
                self.is_done = true;
            } else {
                assert!(
                    ending_cursor <= u32::MAX as i64 && ending_cursor > self.cursor as i64,
                    "fill_buf set invalid ending_cursor: {}, cursor: {}, nwritten: {}",
                    ending_cursor,
                    self.cursor,
                    nwritten
                );
                // otherwise adjust the cursor for the next fill
                self.cursor = ending_cursor as u32;
            }
        }
        // Find the index of the first terminator byte in the buffer, or panic. A missing
        // terminator violates the protocol of these hostcalls, which must always terminate each
        // element with the terminator byte.
        let first_term_ix = self
            .buf
            .iter()
            .position(|b| b == &self.term)
            .expect("terminator byte was not found");
        // split off the first element from the buffer
        let elt = self.buf.split_to(first_term_ix);
        // drop the terminator byte, which now remains in the buffer
        self.buf.advance(1);
        Some(Ok(elt.freeze()))
    }
}

impl<F> std::iter::FusedIterator for MultiValueHostcall<F> where
    F: Fn(*mut u8, usize, u32, *mut i64, *mut usize) -> FastlyStatus
{
}
