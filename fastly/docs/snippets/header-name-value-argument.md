
# Argument type conversion

The header name and value arguments can be any types that implement [`ToHeaderName`] and
[`ToHeaderValue`], respectively. See those traits for details on which types can be used and when
panics may arise during conversion.

