
This operation is potentially expensive if the body is large. Take care when using this
method on bodies with unknown sizes. Consider using methods like [`BufRead::lines()`] or
[`Body::read_chunks()`] to incrementally process a body while limiting the maximum size.

