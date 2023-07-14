//! Purging operations for Compute@Edge.
//!
//! See the [Fastly purge documentation][doc] for details.
//!
//! [doc]: https://developer.fastly.com/learning/concepts/purging/
use fastly_sys::fastly_purge as sys;

use anyhow::anyhow;

use crate::Error;

/// Purge a surrogate key for the current service.
///
/// See the [Fastly purge documentation][doc] for details.
///
/// [doc]: https://developer.fastly.com/learning/concepts/purging/
pub fn purge_surrogate_key(surrogate_key: &str) -> Result<(), Error> {
    purge_surrogate_key_impl(surrogate_key, false)
}

/// Soft-purge a surrogate key for the current service.
///
/// See the [Fastly purge documentation][doc] for details.
///
/// [doc]: https://developer.fastly.com/learning/concepts/purging/
pub fn soft_purge_surrogate_key(surrogate_key: &str) -> Result<(), Error> {
    purge_surrogate_key_impl(surrogate_key, true)
}

fn purge_surrogate_key_impl(surrogate_key: &str, soft: bool) -> Result<(), Error> {
    let mut options_mask = sys::PurgeOptionsMask::empty();
    options_mask.set(sys::PurgeOptionsMask::SOFT_PURGE, soft);
    // This struct will be unused for now since we're not setting the RET_BUF bit in the mask, but
    // we have to pass something here.
    let mut options = sys::PurgeOptions {
        ret_buf_ptr: std::ptr::null_mut(),
        ret_buf_len: 0,
        ret_buf_nwritten_out: std::ptr::null_mut(),
    };
    unsafe {
        sys::purge_surrogate_key(
            surrogate_key.as_ptr(),
            surrogate_key.len(),
            options_mask,
            &mut options,
        )
    }
    .result()
    .map_err(|e| anyhow!("purge error: {:?}", e))
}
