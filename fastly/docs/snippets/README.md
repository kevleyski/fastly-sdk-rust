# Rustdoc Snippets

These are little bits of Markdown that are included in the Rustdoc of this crate. This allows us to
edit repeated bits of documentation comment in a single place, rather than having to search/replace
the whole crate every time we want to make a consistent change.

```rust
    /// Get a shared reference to the body of this response.
    ///
    #[doc = include_str!("../docs/snippets/creates-empty-body.md")]
    pub fn get_body(&self) -> &Body {
        ...
```

The include mechanism is a little wobbly, particularly when mixing and matching Rustdoc comments and
the snippets. For ease of formatting, you should include a blank line at the beginning and end of
each snippet file, so that they are rendered with at least paragraph separation from the rest of the
comment.
