//! Convenient conversion traits.
//
// This module contains traits which make using the SDK feel a little bit more like using a
// dynamically-typed language. By making our methods take `impl ToHeaderName`, for example, rather
// than `HeaderName`, we allow a variety of types to be used as arguments without burdening the end
// user with a lot of explicit conversions.
//
// These traits are similar in spirit to [`std::convert::TryInto`] but with two key differences:
//
// 1. Where `TryInto` has an associated error type, we instead panic when a conversion fails. The
// documentation for each trait describes which conversions can fail, and we encourage using
// explicit conversions with error handling when the source value is untrusted.
//
// 2. Some complicated trait design is used so that if a method does not need an owned value, it
// can borrow the value passed in and avoid a clone. Luckily, we are able to use sealed traits to
// hide this complexity from the end user.
#[macro_use]
mod macros;

use crate::backend::Backend;
use ::url::Url;
use http::header::{HeaderName, HeaderValue};
use http::{Method, StatusCode};

pub use self::backend::ToBackend;
pub(crate) use self::borrowable::Borrowable;
pub use self::header_name::ToHeaderName;
pub use self::header_value::ToHeaderValue;
pub use self::method::ToMethod;
pub use self::status_code::ToStatusCode;
pub use self::url::ToUrl;

mod borrowable {
    pub trait Borrowable<T> {
        fn as_ref(&self) -> &T;
    }
}

mod header_name {
    use super::*;

    /// Types that can be converted to a [`HeaderName`].
    ///
    /// Some methods in this crate accept `impl ToHeaderName` arguments. Any of the types below can
    /// be passed as those arguments and the conversion will be performed automatically, though
    /// depending on the type, the conversion can panic.
    ///
    /// | Source type                                                | Can panic? | Non-panicking conversion   |
    /// |------------------------------------------------------------|------------|----------------------------|
    /// | [`HeaderName` or `&HeaderName`][`HeaderName`]              | No         | N/A                        |
    /// | [`HeaderValue` or `&HeaderValue`][`HeaderValue`]           | Yes        | [`HeaderName::try_from()`] |
    /// | [`&str`][`str`], [`String`, or `&String`][`String`]        | Yes        | [`HeaderName::try_from()`] |
    /// | [`&[u8]`][`std::slice`], [`Vec<u8>`, or `&Vec<u8>`][`Vec`] | Yes        | [`HeaderName::try_from()`] |
    ///
    #[doc = include_str!("../docs/snippets/conversion-may-panic.md")]
    pub trait ToHeaderName: Sealed {}

    convert_stringy!(
        @with_byte_impls,
        HeaderName,
        ToHeaderName,
        Sealed,
        "invalid HTTP header name",
        std::fmt::Debug,
        std::fmt::Display
    );

    impl ToHeaderName for HeaderValue {}
    impl ToHeaderName for &HeaderValue {}

    impl Sealed for HeaderValue {
        type Borrowable = HeaderName;

        fn into_borrowable(self) -> Self::Borrowable {
            Sealed::into_borrowable(self.as_bytes())
        }

        fn into_owned(self) -> HeaderName {
            Sealed::into_owned(self.as_bytes())
        }
    }

    impl Sealed for &HeaderValue {
        type Borrowable = HeaderName;

        fn into_borrowable(self) -> Self::Borrowable {
            Sealed::into_borrowable(self.as_bytes())
        }

        fn into_owned(self) -> HeaderName {
            Sealed::into_owned(self.as_bytes())
        }
    }
}

mod header_value {
    use super::*;

    /// Types that can be converted to a [`HeaderValue`].
    ///
    /// Some methods in this crate accept `impl ToHeaderValue` arguments. Any of the types below can
    /// be passed as those arguments and the conversion will be performed automatically, though
    /// depending on the type, the conversion can panic.
    ///
    /// | Source type                                                | Can panic? | Non-panicking conversion    |
    /// |------------------------------------------------------------|------------|-----------------------------|
    /// | [`HeaderName` or `&HeaderName`][`HeaderName`]              | No         | N/A                         |
    /// | [`HeaderValue` or `&HeaderValue`][`HeaderValue`]           | No         | N/A                         |
    /// | [`Url or &Url`][`Url`]                                     | No         | N/A                         |
    /// | [`&str`][`str`], [`String`, or `&String`][`String`]        | Yes        | [`HeaderValue::try_from()`] |
    /// | [`&[u8]`][`std::slice`], [`Vec<u8>`, or `&Vec<u8>`][`Vec`] | Yes        | [`HeaderValue::try_from()`] |
    ///
    #[doc = include_str!("../docs/snippets/conversion-may-panic.md")]
    pub trait ToHeaderValue: Sealed {}

    convert_stringy!(
        @with_byte_impls,
        HeaderValue,
        ToHeaderValue,
        Sealed,
        "invalid HTTP header value",
        std::fmt::Debug
    );

    impl ToHeaderValue for HeaderName {}
    impl ToHeaderValue for &HeaderName {}
    impl ToHeaderValue for Url {}
    impl ToHeaderValue for &Url {}

    impl Sealed for HeaderName {
        type Borrowable = HeaderValue;

        fn into_borrowable(self) -> Self::Borrowable {
            HeaderValue::from(self)
        }

        fn into_owned(self) -> HeaderValue {
            HeaderValue::from(self)
        }
    }

    impl Sealed for &HeaderName {
        type Borrowable = HeaderValue;

        fn into_borrowable(self) -> Self::Borrowable {
            HeaderValue::from(self.clone())
        }

        fn into_owned(self) -> HeaderValue {
            HeaderValue::from(self.clone())
        }
    }

    impl Sealed for Url {
        type Borrowable = HeaderValue;

        fn into_borrowable(self) -> Self::Borrowable {
            self.to_string().into_borrowable()
        }

        fn into_owned(self) -> HeaderValue {
            self.to_string().into_owned()
        }
    }

    impl Sealed for &Url {
        type Borrowable = HeaderValue;

        fn into_borrowable(self) -> Self::Borrowable {
            self.as_str().into_borrowable()
        }

        fn into_owned(self) -> HeaderValue {
            self.as_str().into_owned()
        }
    }
}

mod method {
    use super::*;

    /// Types that can be converted to a [`Method`].
    ///
    /// Some methods in this crate accept `impl ToMethod` arguments. Any of the types below can be
    /// passed as those arguments and the conversion will be performed automatically, though
    /// depending on the type, the conversion can panic.
    ///
    /// | Source type                                                | Can panic? | Non-panicking conversion |
    /// |------------------------------------------------------------|------------|--------------------------|
    /// | [`Method` or `&Method`][`Method`]                          | No         | N/A                      |
    /// | [`&str`][`str`], [`String`, or `&String`][`String`]        | Yes        | [`Method::try_from()`]   |
    /// | [`&[u8]`][`std::slice`], [`Vec<u8>`, or `&Vec<u8>`][`Vec`] | Yes        | [`Method::try_from()`]   |
    ///
    #[doc = include_str!("../docs/snippets/conversion-may-panic.md")]
    pub trait ToMethod: Sealed {}

    convert_stringy!(
        @with_byte_impls,
        Method,
        ToMethod,
        Sealed,
        "invalid HTTP method",
        std::fmt::Debug,
        std::fmt::Display
    );
}

mod url {
    use super::*;

    /// Types that can be converted to a [`Url`].
    ///
    /// Some methods in this crate accept `impl ToUrl` arguments. Any of the types below can be
    /// passed as those arguments and the conversion will be performed automatically, though
    /// depending on the type, the conversion can panic.
    ///
    /// | Source type                                         | Can panic? | Non-panicking conversion |
    /// |-----------------------------------------------------|------------|--------------------------|
    /// | [`Url or &Url`][`Url`]                              | No         | N/A                      |
    /// | [`&str`][`str`], [`String`, or `&String`][`String`] | Yes        | [`Url::parse()`]         |
    ///
    #[doc = include_str!("../docs/snippets/conversion-may-panic.md")]
    pub trait ToUrl: Sealed {}

    convert_stringy!(
        Url,
        ToUrl,
        Sealed,
        "invalid URL",
        std::fmt::Debug,
        std::fmt::Display
    );
}

mod status_code {
    use super::*;

    /// Types that can be converted to a [`StatusCode`].
    ///
    /// Some methods in this crate accept `impl ToStatusCode` arguments. Any of the types below can
    /// be passed as those arguments and the conversion will be performed automatically, though
    /// depending on the type, the conversion can panic.
    ///
    /// | Source type    | Can panic? | Non-panicking conversion   |
    /// |----------------|------------|----------------------------|
    /// | [`StatusCode`] | No         | N/A                        |
    /// | [`u16`]        | Yes        | [`StatusCode::try_from()`] |
    ///
    #[doc = include_str!("../docs/snippets/conversion-may-panic.md")]
    pub trait ToStatusCode: Sealed {}

    impl ToStatusCode for StatusCode {}

    impl ToStatusCode for u16 {}

    pub trait Sealed {
        fn to_status_code(self) -> StatusCode;
    }

    impl Sealed for StatusCode {
        fn to_status_code(self) -> StatusCode {
            self
        }
    }

    impl Sealed for u16 {
        fn to_status_code(self) -> StatusCode {
            StatusCode::from_u16(self)
                .unwrap_or_else(|_| panic!("invalid HTTP status code: {}", self))
        }
    }
}

mod backend {
    use super::*;

    /// Types that can be converted to a [`Backend`].
    ///
    /// Some methods in this crate accept `impl ToBackend` arguments. Any of the types below can be
    /// passed as those arguments and the conversion will be performed automatically, though
    /// depending on the type, the conversion can panic.
    ///
    /// | Source type                                         | Can panic? | Non-panicking conversion |
    /// |-----------------------------------------------------|------------|--------------------------|
    /// | [`Backend or &Backend`][`Backend`]                  | No         | N/A                      |
    /// | [`&str`][`str`], [`String`, or `&String`][`String`] | Yes        | [`Backend::from_name()`] |
    ///
    #[doc = include_str!("../docs/snippets/conversion-may-panic.md")]
    pub trait ToBackend: Sealed {}

    convert_stringy!(
        Backend,
        ToBackend,
        Sealed,
        "invalid backend",
        std::fmt::Debug
    );
}
