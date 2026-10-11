fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" {
        // Clean library-test checkouts do not yet have the release helper.
        // Packaging builds the real executable before invoking Tauri.
        let resource = std::path::Path::new("resources/minerdesk-linux-helper");
        if !resource.exists() {
            std::fs::create_dir_all("resources").expect("create Linux resources");
            std::fs::write(resource, b"Build with scripts/prepare-linux-helper.mjs before packaging\n").expect("create non-executable placeholder");
        }
    }

    let mut attrs = tauri_build::Attributes::new();

    if target_os == "windows" {
        let windows = tauri_build::WindowsAttributes::new()
            .app_manifest(include_str!("windows-app-manifest.xml"));
        attrs = attrs.windows_attributes(windows);
    }

    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}
