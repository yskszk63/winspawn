extern crate proc_macro;

use proc_macro::TokenStream;

use quote::quote;
use syn::{ItemFn, parse_macro_input};

#[proc_macro_attribute]
pub fn ignore_invalid_handler(_: TokenStream, item: TokenStream) -> TokenStream {
    // TODO symbol resolve rule
    let fun = parse_macro_input!(item as ItemFn);

    let ItemFn { attrs, vis, sig, block } = fun;
    let result = quote! {
        #(#attrs)*
        #vis #sig {
            unsafe extern "C" fn handler(_: *const wchar_t, _: *const wchar_t, _: *const wchar_t, _: c_uint, _: usize) {}

            let old_handler = unsafe {
                _set_thread_local_invalid_parameter_handler(Some(handler))
            };

            let r = #block;

            unsafe {
                _set_thread_local_invalid_parameter_handler(old_handler)
            };

            r
        }
    };
    result.into()
}
