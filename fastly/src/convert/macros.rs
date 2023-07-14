//! Macros to support the implementation of conversion traits.

/// The main implementation macro for conversion traits.
///
/// See `convert.rs` for examples of this macro in use.
///
/// The arguments are:
///
/// - `@with_byte_impls`: an optional token which indicates that byte slice and vector trait impls
///   should be generated. If present, it must be the first argument.
///
/// - `$type`: the target type of the conversion traits. For example, this is `HeaderName` when
///   implementing `ToHeaderName`.
///
/// - `$trait`: the name of the user-facing trait being implemented, for example
///   `ToHeaderName`. This should be a trait with no methods that inherits from `$sealed`, where the
///    actual conversion methods exist. This trait is not defined in this macro expansion so that it
///    may be documented more easily.
///
/// - `$sealed`: the name of the supertrait that the caller gave to `$trait`. This is usually just
///   `Sealed`. This trait _is_ defined in the macro expansion, and so should not be defined by the
///   caller.
///
/// - `$fail_msg`: a human-readable message to use when conversion fails, for example `"invalid HTTP
///   header name".
///
/// - `$extra_bound*`: zero or more additional trait bounds that can be useful to apply to `$type`,
///   commonly `std::fmt::Debug` and `std::fmt::Display`.
///
/// The recommended pattern is to create a one-off module for each conversion trait, use the macro
/// in that module's scope, hand-write any non-standard implementations of `$sealed`, and then
/// export only `$trait`. This prevents other code from adding implementations of the trait or using
/// it as a monomorphic substitute for [`std::convert::Into`].
macro_rules! convert_stringy {
    ( $type:path, $trait:ident, $sealed:ident, $fail_msg:literal $(, $extra_bound:path )* ) => {
        #[allow(unused)]
        use std::str::FromStr;

        impl Borrowable<$type> for $type {
            fn as_ref(&self) -> &$type {
                self
            }
        }

        impl Borrowable<$type> for &$type {
            fn as_ref(&self) -> &$type {
                &*self
            }
        }

        impl $trait for $type {}
        impl<'a> $trait for &'a $type {}
        impl $trait for &str {}
        impl $trait for String {}
        impl $trait for &String {}

        pub trait $sealed {
            type Borrowable: Borrowable<$type> $(+ $extra_bound )*;

            fn into_borrowable(self) -> Self::Borrowable;
            fn into_owned(self) -> $type;
        }

        impl $sealed for $type {
            type Borrowable = $type;

            fn into_borrowable(self) -> Self::Borrowable {
                self
            }

            fn into_owned(self) -> $type {
                self
            }
        }

        impl<'a> $sealed for &'a $type {
            type Borrowable = &'a $type;

            fn into_borrowable(self) -> Self::Borrowable {
                self
            }

            fn into_owned(self) -> $type {
                self.clone()
            }
        }

        impl $sealed for &str {
            type Borrowable = $type;

            fn into_borrowable(self) -> Self::Borrowable {
                <$type>::from_str(self).unwrap_or_else(|_| panic!(concat!($fail_msg, ": {}"), self))
            }

            fn into_owned(self) -> $type {
                $sealed::into_borrowable(self)
            }
        }

        impl $sealed for String {
            type Borrowable = $type;

            fn into_borrowable(self) -> Self::Borrowable {
                $sealed::into_borrowable(self.as_str())
            }

            fn into_owned(self) -> $type {
                $sealed::into_owned(self.as_str())
            }
        }

        impl $sealed for &String {
            type Borrowable = $type;

            fn into_borrowable(self) -> Self::Borrowable {
                $sealed::into_borrowable(self.as_str())
            }

            fn into_owned(self) -> $type {
                $sealed::into_owned(self.as_str())
            }
        }
    };
    ( @with_byte_impls, $type:path, $trait:ident, $sealed:ident, $fail_msg:literal $(, $extra_bound:path )* ) => {
        convert_stringy!($type, $trait, $sealed, $fail_msg $(, $extra_bound )*);

        impl $trait for &[u8] {}
        impl $trait for Vec<u8> {}
        impl $trait for &Vec<u8> {}

        impl $sealed for &[u8] {
            type Borrowable = $type;

            fn into_borrowable(self) -> Self::Borrowable {
                <$type>::try_from(self).unwrap_or_else(|_| panic!(concat!($fail_msg, ": {:?}"), self))
            }

            fn into_owned(self) -> $type {
                $sealed::into_borrowable(self)
            }
        }

        impl $sealed for Vec<u8> {
            type Borrowable = $type;

            fn into_borrowable(self) -> Self::Borrowable {
                $sealed::into_borrowable(self.as_slice())
            }

            fn into_owned(self) -> $type {
                $sealed::into_owned(self.as_slice())
            }
        }

        impl $sealed for &Vec<u8> {
            type Borrowable = $type;

            fn into_borrowable(self) -> Self::Borrowable {
                $sealed::into_borrowable(self.as_slice())
            }

            fn into_owned(self) -> $type {
                $sealed::into_owned(self.as_slice())
            }
        }
    };
}
