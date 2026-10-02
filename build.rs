fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    // `draw_soft`: the software drawing backend (`backends/draw/soft`) is the one compiled for this
    // target. Mirrors the `mod backend` switch in `backends/draw/mod.rs`; derived from the target
    // OS only, never from a feature.
    println!("cargo::rustc-check-cfg=cfg(draw_soft)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if os != "windows" && os != "macos" {
        println!("cargo::rustc-cfg=draw_soft");
    }
    // link.exe flags: GNU ld would take them for input files.
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let manifest_path = format!("{}/app.manifest", manifest_dir);
        println!("cargo:rustc-link-arg-examples=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-examples=/MANIFESTINPUT:{}", manifest_path);
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}", manifest_path);
    }
}
