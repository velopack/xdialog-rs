fn main() {
    #[cfg(target_os = "windows")]
    link_mshtml();

    #[cfg(target_os = "windows")]
    link_winres();
}

#[cfg(target_os = "windows")]
fn link_mshtml() {
    let target = std::env::var("TARGET").unwrap();
    let mut build = cc::Build::new();

    build.include("src/sys/mshtml/webview.h").flag_if_supported("-std=c11").flag_if_supported("-w");

    if std::env::var("DEBUG").is_err() {
        build.define("NDEBUG", None);
    } else {
        build.define("DEBUG", None);
    }

    if target.contains("windows") {
        build.define("UNICODE", None);
        build.file("src/sys/mshtml/webview_mshtml.c");

        for &lib in &["ole32", "comctl32", "oleaut32", "uuid", "gdi32", "user32"] {
            println!("cargo:rustc-link-lib={}", lib);
        }
    } else {
        panic!("unsupported target");
    }

    build.compile("mshtml_webview");
}

#[cfg(target_os = "windows")]
fn link_winres() {
    winres::WindowsResource::new()
        .set_manifest_file("app.manifest")
        // .set_version_info(winres::VersionInfo::PRODUCTVERSION, ver)
        // .set_version_info(winres::VersionInfo::FILEVERSION, ver)
        .set("CompanyName", "Velopack")
        .set("ProductName", "Velopack")
        // .set("ProductVersion", &version)
        // .set("FileDescription", &desc)
        .set("LegalCopyright", "Caelan Sayler (c) 2023, Velopack Ltd. (c) 2024")
        .compile()
        .unwrap();
}
