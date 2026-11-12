use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn compiler_below(directory: &Path) -> Option<PathBuf> {
    // rc.exe is a host tool; its .res output is independent of the target CPU.
    let host = match std::env::var("HOST").unwrap_or_default().split('-').next() {
        Some("aarch64") => "arm64",
        Some("i686") => "x86",
        _ => "x64",
    };
    for candidate in [
        directory.join("rc.exe"),
        directory.join(host).join("rc.exe"),
        directory.join("x64/rc.exe"),
        directory.join("x86/rc.exe"),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn compiler_in_kit(root: &Path) -> Option<PathBuf> {
    let bin = root.join("bin");
    // Prefer the newest fully installed SDK. Partial/older SDK directories
    // without a compiler are skipped rather than masking the next candidate.
    let mut versions: Vec<_> = std::fs::read_dir(&bin)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let parts: Option<Vec<u32>> = name
                .to_str()?
                .split('.')
                .map(|part| part.parse().ok())
                .collect();
            let parts = parts?;
            (parts.len() >= 3).then_some((parts, entry.path()))
        })
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, version) in versions {
        if let Some(compiler) = compiler_below(&version) {
            return Some(compiler);
        }
    }
    compiler_below(&bin)
}

fn resource_compiler() -> PathBuf {
    for name in ["RC", "WindowsSdkVerBinPath", "WindowsSdkBinPath"] {
        if let Some(value) = std::env::var_os(name) {
            let path = PathBuf::from(value);
            if name == "RC" && path.is_file() {
                return path;
            }
            if let Some(compiler) = compiler_below(&path) {
                return compiler;
            }
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            if let Some(compiler) = compiler_below(&directory) {
                return compiler;
            }
        }
    }
    let mut roots = Vec::new();
    if let Some(path) = std::env::var_os("WindowsSdkDir") {
        roots.push(PathBuf::from(path));
    }
    // GitHub Windows runners have the SDK installed without a Developer Shell.
    // Read its machine registration through the OS tool, requiring no crate or
    // Visual Studio environment initialization.
    let windows = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let registry = windows.join("System32/reg.exe");
    for view in ["/reg:64", "/reg:32"] {
        if let Ok(output) = Command::new(&registry)
            .args([
                "query",
                r"HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots",
                "/v",
                "KitsRoot10",
                view,
            ])
            .output()
        {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    if let Some((_, value)) = line.split_once("REG_SZ") {
                        let value = value.trim();
                        if !value.is_empty() {
                            roots.push(PathBuf::from(value));
                        }
                    }
                }
            }
        }
    }
    for name in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(path) = std::env::var_os(name) {
            roots.push(PathBuf::from(path).join("Windows Kits/10"));
        }
    }
    roots.extend([
        PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10"),
        PathBuf::from(r"C:\Program Files\Windows Kits\10"),
    ]);
    for root in roots {
        if let Some(compiler) = compiler_in_kit(&root) {
            return compiler;
        }
    }
    panic!("Windows SDK rc.exe was not found. Install a Windows 10/11 SDK, or set RC to its compiler executable.");
}

fn main() {
    for name in [
        "RC",
        "WindowsSdkVerBinPath",
        "WindowsSdkBinPath",
        "WindowsSdkDir",
        "PATH",
        "ProgramFiles(x86)",
        "ProgramFiles",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = env!("CARGO_PKG_VERSION");
        let parts: Vec<u16> = version
            .split('.')
            .map(|p| p.parse().expect("numeric worker resource version"))
            .collect();
        let numeric = format!("{},{},{},0", parts[0], parts[1], parts[2]);
        let directory =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("worker resource output"));
        let source = directory.join("minerdesk-update-helper.rc");
        let resource = directory.join("minerdesk-update-helper.res");
        std::fs::write(
            &source,
            format!(
                r#"1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004L
FILETYPE 1L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "ProductName", "MinerDesk\0"
      VALUE "FileDescription", "MinerDesk signed Fleet update worker\0"
      VALUE "OriginalFilename", "minerdesk-update-helper.exe\0"
      VALUE "InternalName", "minerdesk-update-helper\0"
      VALUE "ProductVersion", "{version}\0"
      VALUE "FileVersion", "{version}\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#
            ),
        )
        .expect("write worker VERSIONINFO resource");
        let compiler = resource_compiler();
        println!("cargo:rerun-if-changed={}", compiler.display());
        let status = Command::new(&compiler)
            .arg("/nologo")
            .arg("/fo")
            .arg(&resource)
            .arg(&source)
            .status()
            .expect("Windows SDK rc.exe is required for worker version metadata");
        assert!(
            status.success(),
            "compile update worker Windows version metadata"
        );
        println!(
            "cargo:rustc-link-arg-bin=minerdesk-update-helper={}",
            resource.display()
        );
    }
}
