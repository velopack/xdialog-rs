extern crate cc;
extern crate pkg_config;

use std::env;

fn main() {
    let target = env::var("TARGET").unwrap();

    let mut build = cc::Build::new();

    build
        .include("webview.h")
        .flag_if_supported("-std=c11")
        .flag_if_supported("-w");

    if env::var("DEBUG").is_err() {
        build.define("NDEBUG", None);
    } else {
        build.define("DEBUG", None);
    }

    if target.contains("windows") {
        build.define("UNICODE", None);
        build.file("webview_mshtml.c");

        for &lib in &["ole32", "comctl32", "oleaut32", "uuid", "gdi32", "user32"] {
            println!("cargo:rustc-link-lib={}", lib);
        }
    } else {
        panic!("unsupported target");
    }

    build.compile("webview");
}
