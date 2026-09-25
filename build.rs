fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let manifest_path = format!("{}/app.manifest", manifest_dir);
        println!("cargo:rustc-link-arg-examples=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-examples=/MANIFESTINPUT:{}", manifest_path);
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}", manifest_path);
    }

    emit_cfg_aliases();
}

/// Cargo features cannot be target-conditional, so the effective backend configuration is
/// computed here from the target OS and the enabled features, and exposed as `xd_*` cfgs
/// Code must use these aliases rather than the raw features.
fn emit_cfg_aliases() {
    const ALIASES: &[&str] = &["xd_egui",
                               "xd_own_loop",
                               "xd_theme_ubuntu",
                               "xd_theme_fluent",
                               "xd_linux_direct",
                               "xd_winit_host",
                               "xd_winit_host_stub",
                               "xd_test_hooks"];
    for alias in ALIASES {
        println!("cargo::rustc-check-cfg=cfg({})", alias);
    }
    println!("cargo::rustc-check-cfg=cfg(docsrs)");

    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let linux = os == "linux";
    let windows = os == "windows";
    let macos = os == "macos";
    let desktop = linux || windows;

    // `_egui` -> CARGO_FEATURE__EGUI, `_ubuntu-theme` -> CARGO_FEATURE__UBUNTU_THEME, etc.
    let feature = |name: &str| std::env::var_os(format!("CARGO_FEATURE_{}", name)).is_some();
    let f_egui = feature("_EGUI");
    let f_ubuntu_theme = feature("_UBUNTU_THEME");
    let f_fluent_theme = feature("_FLUENT_THEME");
    let f_builtin_winit = feature("BUILTIN_WINIT");
    let f_linux_direct = feature("LINUX_DIRECT");
    let f_winit_host = feature("WINIT_HOST");
    let f_test_hooks = feature("_TEST_HOOKS");

    let xd_egui = desktop && (linux || f_egui);
    let xd_own_loop = desktop && f_builtin_winit && (linux || f_ubuntu_theme || f_fluent_theme);
    let xd_theme_ubuntu = desktop && (linux || f_ubuntu_theme);
    let xd_theme_fluent = desktop && f_fluent_theme;
    let xd_linux_direct = xd_own_loop && f_linux_direct;
    let xd_winit_host = desktop && f_winit_host;
    let xd_winit_host_stub = macos && f_winit_host;
    let xd_test_hooks = desktop && f_test_hooks;

    let set = |on: bool, name: &str| {
        if on {
            println!("cargo:rustc-cfg={}", name);
        }
    };
    set(xd_egui, "xd_egui");
    set(xd_own_loop, "xd_own_loop");
    set(xd_theme_ubuntu, "xd_theme_ubuntu");
    set(xd_theme_fluent, "xd_theme_fluent");
    set(xd_linux_direct, "xd_linux_direct");
    set(xd_winit_host, "xd_winit_host");
    set(xd_winit_host_stub, "xd_winit_host_stub");
    set(xd_test_hooks, "xd_test_hooks");
}
