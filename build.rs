fn main() {
    #[cfg(target_os = "windows")]
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
