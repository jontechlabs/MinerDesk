fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    let mut attrs = tauri_build::Attributes::new();

    if target_os == "windows" {
        let windows = tauri_build::WindowsAttributes::new()
            .app_manifest(include_str!("windows-app-manifest.xml"));
        attrs = attrs.windows_attributes(windows);
    }

    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}
