// Warnings (other than unused variables) in doctests are promoted to errors.
#![doc(test(attr(deny(warnings))))]
#![doc(test(attr(allow(dead_code))))]
#![doc(test(attr(allow(unused_variables))))]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::invalid_codeblock_attributes)]

//! Implementation detail of the `fastly` crate.

extern crate proc_macro;
use {
    proc_macro::TokenStream,
    proc_macro2::Span,
    quote::quote_spanned,
    syn::{
        parse_macro_input, parse_quote, punctuated::Punctuated, spanned::Spanned, Attribute, Ident,
        ItemFn, ReturnType, Signature, Visibility,
    },
};

/// Main function attribute for a Compute@Edge program.
///
/// ## Usage
///
/// This attribute should be applied to a `main` function that takes a request and returns a
/// response or an error. For example:
///
/// ```rust,no_run
/// use fastly::{Error, Request, Response};
///
/// #[fastly::main]
/// fn main(ds_req: Request) -> Result<Response, Error> {
///     Ok(ds_req.send("example_backend")?)
/// }
/// ```
///
/// You can apply `#[fastly::main]` to any function that takes `Request` as its sole argument, and
/// returns a `Result<Response, Error>`.
///
/// ## More Information
///
/// This is a convenience to abstract over the common usage of `Request::from_client()` and
/// `Response::send_to_client()` at the beginning and end of a program's `main()` function. The
/// macro use above is equivalent to the following code:
///
/// ```rust,no_run
/// use fastly::{Error, Request};
///
/// fn main() -> Result<(), Error> {
///     let ds_req = Request::from_client();
///     let us_resp = ds_req.send("example_backend")?;
///     us_resp.send_to_client();
///     Ok(())
/// }
/// ```
#[proc_macro_attribute]
pub fn main(_: TokenStream, input: TokenStream) -> TokenStream {
    // Parse the input token stream as a free-standing function, or return an error.
    let raw_main = parse_macro_input!(input as ItemFn);

    // Check that the function signature looks okay-ish. If we have the wrong number of arguments,
    // or no return type is specified , print a friendly spanned error with the expected signature.
    if !check_impl_signature(&raw_main.sig) {
        return syn::Error::new(
            raw_main.sig.span(),
            "`fastly::main` expects a function such as:

#[fastly::main]
fn main (request: Request) -> Result<Response, Error> {
    ...
}
",
        )
        .to_compile_error()
        .into();
    }

    // Get the attributes, visibility, and signature of our outer function. Then, update the
    // attributes and visibility of the inner function that we will inline.
    let (attrs, vis, sig) = outer_main_info(&raw_main);
    let (name, inner_fn) = inner_fn_info(raw_main);

    // Define our raw main function, which will provide the downstream request to our main function
    // implementation as its argument, and then send the `ResponseExt` result downstream.
    let output = quote_spanned! {inner_fn.span() =>
        #(#attrs)*
        #vis
        #sig {
            #[inline(always)]
            #inner_fn
            fastly::init();
            let ds_req = fastly::Request::from_client();
            match #name(ds_req) {
                Ok(ds_resp) => ds_resp.send_to_client(),
                Err(e) => {
                    fastly::Response::from_body(e.to_string())
                        .with_status(fastly::http::StatusCode::INTERNAL_SERVER_ERROR)
                        .send_to_client()
                }
            };
            Ok(())
        }
    };

    output.into()
}

/// Check if the signature of the `#[main]` function seems correct.
///
/// Unfortunately, we cannot precisely typecheck in a procedural macro attribute, because we are
/// dealing with [`TokenStream`]s. This checks that our signature takes one input, and has a return
/// type. Specific type errors are caught later, after the [`fastly_main`] macro has been expanded.
///
/// This is used by the [`fastly_main`] procedural macro attribute to help provide friendly errors
/// when given a function with the incorrect signature.
///
/// [`fastly_main`]: attr.fastly_main.html
/// [`TokenStream`]: proc_macro/struct.TokenStream.html
fn check_impl_signature(sig: &Signature) -> bool {
    if sig.inputs.iter().len() != 1 {
        false // Return false if the signature takes no inputs, or more than one input.
    } else if let ReturnType::Default = sig.output {
        false // Return false if the signature's output type is empty.
    } else {
        true
    }
}

/// Returns a 3-tuple containing the attributes, visibility, and signature of our outer `main`.
///
/// The outer main function will use the same attributes and visibility as our raw main function.
///
/// The signature of the outer function will be changed to have inputs and outputs of the form
/// `fn main() -> Result<(), fastly::Error>`. The name of the outer main will always be just that,
/// `main`.
fn outer_main_info(inner_main: &ItemFn) -> (Vec<Attribute>, Visibility, Signature) {
    let attrs = inner_main.attrs.clone();
    let vis = Visibility::Inherited;
    let sig = {
        let mut sig = inner_main.sig.clone();
        sig.ident = Ident::new("main", Span::call_site());
        sig.inputs = Punctuated::new();
        sig.output = parse_quote!(-> ::std::result::Result<(), fastly::Error>);
        sig
    };

    (attrs, vis, sig)
}

/// Prepare our inner function to be inlined into our main function.
///
/// This changes its visibility to [`Inherited`], and removes [`no_mangle`] from the attributes of
/// the inner function if it is there.
///
/// This function returns a 2-tuple of the inner function's identifier, and the function itself.
/// This identifier is used to emit code calling this function in our `main`.
///
/// [`Inherited`]: syn/enum.Visibility.html#variant.Inherited
/// [`no_mangle`]: https://doc.rust-lang.org/reference/abi.html#the-no_mangle-attribute
fn inner_fn_info(mut inner_main: ItemFn) -> (Ident, ItemFn) {
    let name = inner_main.sig.ident.clone();
    inner_main.vis = Visibility::Inherited;
    inner_main
        .attrs
        .retain(|attr| !attr.path.is_ident("no_mangle"));
    (name, inner_main)
}
