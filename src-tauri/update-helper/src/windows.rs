use crate::*;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{BufReader, Write},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
        process::CommandExt,
    },
    process::{Command, Stdio},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
    Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
    System::Com::CoTaskMemFree,
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        SystemInformation::GetSystemDirectoryW,
        Threading::{
            GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken,
            QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE,
        },
    },
    UI::Shell::{
        CommandLineToArgvW, FOLDERID_ProgramData, FOLDERID_ProgramFiles, SHGetKnownFolderPath,
    },
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn elevated() -> bool {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let token = Handle(token);
        let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
        let mut len = 0;
        GetTokenInformation(
            token.0,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        ) != 0
            && elevation.TokenIsElevated != 0
    }
}
fn process(pid: u32) -> Result<(Handle, PathBuf), String> {
    unsafe {
        let h = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        );
        if h.is_null() {
            return Err("Original backend process is unavailable".into());
        }
        let h = Handle(h);
        let mut buf = vec![0u16; 32768];
        let mut len = buf.len() as u32;
        if QueryFullProcessImageNameW(h.0, 0, buf.as_mut_ptr(), &mut len) == 0 {
            return Err("Cannot identify original backend executable".into());
        }
        Ok((
            h,
            std::ffi::OsString::from_wide(&buf[..len as usize]).into(),
        ))
    }
}
fn parent_pid() -> Result<u32, String> {
    unsafe {
        let h = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if h == INVALID_HANDLE_VALUE {
            return Err("Cannot identify original backend".into());
        }
        let h = Handle(h);
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(h.0, &mut e);
        while ok != 0 {
            if e.th32ProcessID == GetCurrentProcessId() {
                return Ok(e.th32ParentProcessID);
            }
            ok = Process32NextW(h.0, &mut e);
        }
        Err("Missing original backend identity".into())
    }
}
fn same(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}
fn registered(exe: &Path) -> Result<PathBuf, String> {
    use winreg::{enums::*, RegKey};
    let root = RegKey::predef(HKEY_LOCAL_MACHINE);
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        if let Ok(key) = root.open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\MinerDesk",
            KEY_READ | view,
        ) {
            if let Ok(value) = key.get_value::<String, _>("InstallLocation") {
                let value = value.trim();
                let value = if value.starts_with('"') {
                    value
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or("")
                } else {
                    value
                };
                let path = PathBuf::from(value);
                if path.is_absolute()
                    && !value.chars().any(|c| c == '"' || c.is_control())
                    && !path
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                    && same(exe.parent().unwrap_or(Path::new("")), &path)
                {
                    return Ok(path);
                }
            }
        }
    }
    Err("Remote updates require the registered MinerDesk installation".into())
}
fn known_folder(folder: &windows_sys::core::GUID) -> Result<PathBuf, String> {
    unsafe {
        let mut p = std::ptr::null_mut();
        if SHGetKnownFolderPath(folder, 0, std::ptr::null_mut(), &mut p) < 0 || p.is_null() {
            return Err("Windows system directory is unavailable".into());
        }
        let mut len = 0;
        while *p.add(len) != 0 {
            len += 1;
        }
        let path = std::ffi::OsString::from_wide(std::slice::from_raw_parts(p, len));
        CoTaskMemFree(p.cast());
        Ok(PathBuf::from(path))
    }
}
fn program_data() -> Result<PathBuf, String> {
    Ok(known_folder(&FOLDERID_ProgramData)?
        .join("MinerDesk")
        .join("updates"))
}
fn powershell() -> Result<PathBuf, String> {
    unsafe {
        let mut b = vec![0u16; 32768];
        let len = GetSystemDirectoryW(b.as_mut_ptr(), b.len() as u32);
        if len == 0 || len as usize >= b.len() {
            return Err("Windows system directory is unavailable".into());
        }
        Ok(
            PathBuf::from(std::ffi::OsString::from_wide(&b[..len as usize]))
                .join("WindowsPowerShell/v1.0/powershell.exe"),
        )
    }
}
fn clean_environment(command: &mut Command, temp: &Path) -> Result<(), String> {
    let ps = powershell()?;
    let system = ps
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("Missing Windows system directory")?;
    let windows = system.parent().ok_or("Missing Windows directory")?;
    command
        .env_clear()
        .env("SystemRoot", windows)
        .env("WINDIR", windows)
        .env("PATH", system)
        .env("ComSpec", system.join("cmd.exe"))
        .env(
            "PSModulePath",
            system.join("WindowsPowerShell/v1.0/Modules"),
        )
        .env("TEMP", temp)
        .env("TMP", temp)
        .env("ProgramData", known_folder(&FOLDERID_ProgramData)?)
        .env("ProgramFiles", known_folder(&FOLDERID_ProgramFiles)?);
    command.current_dir(system);
    Ok(())
}
fn native(request: serde_json::Value) -> Result<Vec<u8>, String> {
    let script = include_str!("windows.ps1");
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut command = Command::new(powershell()?);
    let temp = program_data()?;
    clean_environment(
        &mut command,
        temp.parent()
            .ok_or("Missing protected MinerDesk directory")?,
    )?;
    let out = command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
        ])
        .arg(STANDARD.encode(utf16))
        .env(
            "MINERDESK_UPDATE_NATIVE_REQUEST",
            STANDARD.encode(serde_json::to_vec(&request).map_err(|e| e.to_string())?),
        )
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "Windows update authorization failed: {}",
            String::from_utf8_lossy(&out.stderr)
                .chars()
                .take(2048)
                .collect::<String>()
        ));
    }
    Ok(out.stdout)
}
pub fn enrolled() -> bool {
    elevated()
}
pub fn helper_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let install = registered(&exe)?;
    let p = install.join("resources/minerdesk-update-helper.exe");
    if p.is_file() {
        Ok(p)
    } else {
        Err("The installed update helper is missing".into())
    }
}
pub fn target_and_reason() -> Result<&'static str, String> {
    if std::env::consts::ARCH != "x86_64" {
        return Err("Remote updates support x86_64 installations".into());
    }
    if !elevated() {
        return Err("Remote updates require the locally authorized privileged backend".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let install = registered(&exe)?;
    if !same(&exe, &install.join("minerdesk-backend.exe")) {
        return Err("Remote updates must run through the installed privileged backend".into());
    }
    helper_path()?;
    Ok("windows-x86_64")
}
pub fn progress_path(job: &str) -> Result<PathBuf, String> {
    if !valid_job_id(job) {
        return Err("Invalid update job identifier".into());
    }
    Ok(program_data()?.join("public").join(format!("{job}.json")))
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    pid: u32,
    creation: String,
    install: PathBuf,
    base: PathBuf,
    version: String,
    task_xml: String,
    command_line: String,
    task_arguments: String,
}
fn arguments(line: &str) -> Result<Vec<std::ffi::OsString>, String> {
    if line.contains('\0') || line.len() > 16384 {
        return Err("Original backend command line is invalid".into());
    }
    let mut wide: Vec<u16> = std::ffi::OsStr::new(line).encode_wide().collect();
    wide.push(0);
    unsafe {
        let mut count = 0;
        let pointer = CommandLineToArgvW(wide.as_ptr(), &mut count);
        if pointer.is_null() || count < 1 {
            return Err("Cannot parse original backend command line".into());
        }
        let mut result = Vec::new();
        for p in std::slice::from_raw_parts(pointer, count as usize) {
            let mut len = 0;
            while *p.add(len) != 0 {
                len += 1;
            }
            result.push(std::ffi::OsString::from_wide(std::slice::from_raw_parts(
                *p, len,
            )));
        }
        LocalFree(pointer.cast());
        Ok(result)
    }
}
fn validate_task_arguments(ctx: &Context) -> Result<(), String> {
    let actual = arguments(&ctx.command_line)?;
    let task = arguments(&format!("minerdesk-backend.exe {}", ctx.task_arguments))?;
    if actual.get(1..) != task.get(1..) {
        return Err("Running backend arguments differ from its registered restart task".into());
    }
    Ok(())
}
fn allocate(path: &Path, public: bool) -> Result<(), String> {
    native(serde_json::json!({"action":"allocate","path":path,"public":public}))?;
    Ok(())
}
fn write_protected(path: &Path, bytes: &[u8], public: bool) -> Result<(), String> {
    let tmp = path.with_file_name(format!(".{}", uuid::Uuid::new_v4()));
    allocate(&tmp, public)?;
    let mut f = OpenOptions::new()
        .write(true)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())?;
    drop(f);
    native(serde_json::json!({"action":"publish","source":tmp,"path":path}))?;
    Ok(())
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_protected(path, bytes, true)
}
fn event(ctx: &Context, req: &Request, stage: &str, error: Option<String>) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(&WorkerEvent {
        schema: 1,
        job_id: req.job_id.clone(),
        stage: stage.into(),
        error,
    })
    .map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    atomic(
        &ctx.base.join("public").join(format!("{}.json", req.job_id)),
        &bytes,
    )?;
    if stage == "ready" {
        std::io::stdout()
            .write_all(&bytes)
            .and_then(|_| std::io::stdout().flush())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn bootstrap() -> Result<(), String> {
    if !elevated() {
        return Err("The update worker requires the previously authorized backend".into());
    }
    let pid = parent_pid()?;
    let (_parent, exe) = process(pid)?;
    let install = registered(&exe)?;
    let context: Context = serde_json::from_slice(&native(
        serde_json::json!({"action":"inspect","pid":pid,"install":install}),
    )?)
    .map_err(|_| "Invalid Windows update context")?;
    validate_task_arguments(&context)?;
    let own = std::env::current_exe().map_err(|e| e.to_string())?;
    if !same(&own, &install.join("resources/minerdesk-update-helper.exe")) {
        return Err("Only the installed update helper may detach a worker".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let directory = context.base.join(format!("worker-{id}"));
    native(
        serde_json::json!({"action":"stage","base":context.base,"directory":directory,"source":own,"context":serde_json::to_string(&context).map_err(|e|e.to_string())?}),
    )?;
    // The first process is only a bootstrap. Explicit breakaway keeps the real
    // worker alive when the backend's crash-guard Job Object closes.
    let mut detached = Command::new(directory.join("minerdesk-update-helper.exe"));
    clean_environment(&mut detached, &directory)?;
    detached
        .args(["--resume", &id])
        .current_dir(&directory)
        .creation_flags(0x01000000 | 0x00000008 | 0x08000000)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Cannot detach the protected update worker: {e}"))?;
    Ok(())
}
fn apply(id: &str) -> Result<(), String> {
    if !valid_job_id(id) || !elevated() {
        return Err("Invalid protected update worker".into());
    }
    let base = program_data()?;
    let directory = base.join(format!("worker-{id}"));
    let own = std::env::current_exe().map_err(|e| e.to_string())?;
    if !same(&own, &directory.join("minerdesk-update-helper.exe")) {
        return Err("Worker is not running from its protected snapshot".into());
    }
    // This directory is below an administrator-only DACL set by bootstrap.
    let ctx: Context = serde_json::from_slice(
        &fs::read(directory.join("context.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|_| "Invalid protected update context")?;
    if ctx.base != base || ctx.install != registered(&ctx.install.join("minerdesk-backend.exe"))? {
        return Err("Protected installation context changed".into());
    }
    native(
        serde_json::json!({"action":"validate","directory":directory,"install":ctx.install,"pid":ctx.pid,"creation":ctx.creation}),
    )?;
    validate_task_arguments(&ctx)?;
    let (parent, exe) = process(ctx.pid)?;
    if !same(&exe, &ctx.install.join("minerdesk-backend.exe")) {
        return Err("Original backend changed".into());
    }
    let mut stdin = BufReader::new(std::io::stdin());
    let req: Request = read_message(&mut stdin)?;
    req.validate()?;
    let result = (|| {
        event(&ctx, &req, "verifying", None)?;
        let artifact =
            official_artifact(&req.expected_version, ctx.version.trim(), "windows-x86_64")?;
        let metadata = fs::symlink_metadata(&req.package_path)
            .map_err(|_| "The downloaded update is unavailable")?;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
            return Err("Downloaded updates cannot be reparse points".into());
        }
        let bytes = regular_bytes(&req.package_path)?;
        verify_package(&bytes, &artifact.signature, &req.expected_version)?;
        if !bytes.starts_with(b"MZ") {
            return Err("Signed update is not a Windows installer".into());
        }
        let package = directory.join("update.exe");
        allocate(&package, false)?;
        let mut out = OpenOptions::new()
            .write(true)
            .open(&package)
            .map_err(|e| e.to_string())?;
        out.write_all(&bytes)
            .and_then(|_| out.sync_all())
            .map_err(|e| e.to_string())?;
        drop(out);
        drop(bytes);
        event(&ctx, &req, "ready", None)?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(read_message::<Ack>(&mut stdin));
        });
        let ack = rx
            .recv_timeout(Duration::from_secs(90))
            .map_err(|_| "Original backend did not acknowledge the update")??;
        if ack.schema != 1 || ack.job_id != req.job_id || !ack.apply {
            return Err("Original backend cancelled the update".into());
        }
        if unsafe { WaitForSingleObject(parent.0, 120_000) } != WAIT_OBJECT_0 {
            return Err("Original backend did not exit; installation cancelled".into());
        }
        finish_after_parent_exit(
            || {
                native(
                    serde_json::json!({"action":"prepare","install":ctx.install,"base":ctx.base}),
                )?;
                native(serde_json::json!({"action":"seal","path":package}))?;
                event(&ctx, &req, "installing", None)?;
                let mut installer = Command::new(&package);
                clean_environment(&mut installer, &directory)?;
                // NSIS requires its final /D= option to remain unquoted, including
                // when the validated registered path contains spaces.
                let status = installer
                    .args(["/S", "/UPDATE"])
                    .raw_arg(format!("/D={}", ctx.install.display()))
                    .current_dir(&directory)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .creation_flags(0x08000000)
                    .status()
                    .map_err(|e| e.to_string())?;
                if !status.success() {
                    return Err("The signed Windows installer failed".into());
                }
                Ok(())
            },
            || event(&ctx, &req, "restarting", None),
            || {
                native(
            serde_json::json!({"action":"restart","install":ctx.install,"task_xml":ctx.task_xml}),
        ).map(|_| ())
            },
        )?;
        event(&ctx, &req, "completed", None)
    })();
    if let Err(error) = &result {
        let _ = write_protected(&directory.join("error.txt"), error.as_bytes(), false);
        let _ = event(
            &ctx,
            &req,
            "failed",
            Some("The signed update failed; inspect the protected local update log".into()),
        );
    }
    result
}
pub fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode] if mode == "--apply" => bootstrap(),
        [mode, id] if mode == "--resume" => apply(id),
        _ => Err("Expected the fixed update apply mode".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(command_line: &str, task_arguments: &str) -> Context {
        Context {
            pid: 123,
            creation: "1".into(),
            install: "C:\\Program Files\\MinerDesk".into(),
            base: "C:\\ProgramData\\MinerDesk\\updates".into(),
            version: "0.8.2".into(),
            task_xml: String::new(),
            command_line: command_line.into(),
            task_arguments: task_arguments.into(),
        }
    }
    #[test]
    fn restart_keeps_the_actual_backend_configuration() {
        let args="--desktop-owned --config \"C:\\Users\\Jon é\\settings.json\" --port 8420 --token local-test-token";
        let ctx = context(
            &format!("\"C:\\Program Files\\MinerDesk\\minerdesk-backend.exe\" {args}"),
            args,
        );
        assert!(validate_task_arguments(&ctx).is_ok());
        let changed = context(&ctx.command_line, "--desktop-owned");
        assert!(validate_task_arguments(&changed).is_err());
    }
    #[test]
    fn restart_does_not_accept_a_changed_token_or_configuration() {
        assert!(
            validate_task_arguments(&context("backend.exe --token original", "--token other"))
                .is_err()
        );
        assert!(validate_task_arguments(&context(
            "backend.exe --config first.json",
            "--config second.json"
        ))
        .is_err());
        assert!(arguments("backend.exe\0 --port 8420").is_err());
    }
}
