use crate::*;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        process::{CommandExt, ExitStatusExt},
    },
    process::{Command, Stdio},
    time::Instant,
};

const BASE: &str = "/var/lib/minerdesk/updates";
const SUDOERS: &str = "/etc/sudoers.d/minerdesk-updates-";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    schema: u32,
    uid: u32,
}

pub fn protected_path(path: &Path, owner: u32, directory: bool) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Unsafe protected path".into());
    }
    let mut walk = PathBuf::from("/");
    for c in path.components() {
        if let std::path::Component::Normal(c) = c {
            walk.push(c);
        }
        let m = fs::symlink_metadata(&walk).map_err(|e| e.to_string())?;
        if m.file_type().is_symlink()
            || (!m.is_dir() && walk != path)
            || m.mode() & 0o022 != 0
            || (m.uid() != 0 && m.uid() != owner)
        {
            return Err("Update path ownership or permissions are unsafe".into());
        }
        if walk == path
            && (m.uid() != owner || (directory && !m.is_dir()) || (!directory && !m.is_file()))
        {
            return Err("Update path has the wrong owner or type".into());
        }
    }
    Ok(())
}
fn root_file(path: &Path) -> Result<(), String> {
    protected_path(path, 0, false)
}
fn create_root_dir(path: &Path, mode: u32) -> Result<(), String> {
    if fs::symlink_metadata(path).is_err() {
        protected_path(path.parent().ok_or("Missing protected parent")?, 0, true)?;
        fs::DirBuilder::new()
            .mode(mode)
            .create(path)
            .map_err(|e| e.to_string())?;
    }
    protected_path(path, 0, true)?;
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if m.mode() & 0o777 != mode {
        return Err("Protected update directory has unexpected permissions".into());
    }
    Ok(())
}
fn atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    if let Ok(m) = fs::symlink_metadata(path) {
        if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0 {
            return Err("Unsafe existing status or grant".into());
        }
    }
    let tmp = path.with_file_name(format!(".{}", uuid::Uuid::new_v4()));
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        out.write_all(bytes).map_err(|e| e.to_string())?;
        out.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        File::open(path.parent().unwrap())
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
fn grant_path(uid: u32) -> PathBuf {
    Path::new(BASE).join("grants").join(format!("{uid}.json"))
}
fn grant(uid: u32) -> Result<(), String> {
    root_file(&grant_path(uid))?;
    let g: Grant = serde_json::from_slice(&regular_bytes(&grant_path(uid))?)
        .map_err(|_| "Invalid update grant")?;
    if g.schema != 1 || g.uid != uid || uid == 0 {
        return Err("Update grant belongs to another user".into());
    }
    Ok(())
}
fn local_uid() -> u32 {
    unsafe { libc::getuid() }
}
pub fn enrolled() -> bool {
    let uid = local_uid();
    // Public grant existence only informs UI; privileged apply reads and verifies
    // the private grant itself and never relies on this observation.
    Path::new(BASE)
        .join("public")
        .join(format!("{uid}-enrolled.json"))
        .is_file()
}
fn appimage() -> Result<PathBuf, String> {
    let path = PathBuf::from(
        std::env::var_os("APPIMAGE")
            .ok_or("This is not an installed Debian package or writable AppImage")?,
    );
    if !path.is_absolute() {
        return Err("Invalid AppImage path".into());
    }
    let m = fs::symlink_metadata(&path).map_err(|_| "AppImage is unavailable")?;
    if !m.is_file()
        || m.uid() != local_uid()
        || m.nlink() != 1
        || m.mode() & 0o022 != 0
        || local_uid() == 0
    {
        return Err("AppImage must be a regular user-owned executable".into());
    }
    protected_path(&path, local_uid(), false)?;
    let parent = path.parent().ok_or("AppImage has no directory")?;
    let probe = parent.join(format!(".minerdesk-write-{}", uuid::Uuid::new_v4()));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&probe)
        .map_err(|_| "AppImage directory is not writable")?;
    fs::remove_file(probe).map_err(|e| e.to_string())?;
    Ok(path)
}
fn installed_version() -> Result<String, String> {
    root_file(Path::new("/usr/bin/dpkg-query"))?;
    let out = Command::new("/usr/bin/dpkg-query")
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .args(["--show", "--showformat=${Status}\n${Version}", "miner-desk"])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8(out.stdout).map_err(|_| "Invalid installed package version")?;
    let version = text
        .strip_prefix("install ok installed\n")
        .ok_or("MinerDesk Debian package is not installed")?;
    if !out.status.success() {
        return Err("MinerDesk Debian package is not installed".into());
    }
    stable_version(version)?;
    Ok(version.into())
}
pub fn target_and_reason() -> Result<&'static str, String> {
    if std::env::consts::ARCH != "x86_64" {
        return Err("Remote updates support x86_64 installations".into());
    }
    if std::env::var_os("APPIMAGE").is_some() {
        appimage()?;
        helper_path()?;
        return Ok("linux-x86_64-appimage");
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if ![
        Path::new("/usr/bin/minerdesk"),
        Path::new("/usr/bin/minerdesk-headless"),
    ]
    .contains(&exe.as_path())
    {
        return Err("Remote Debian updates require the installed MinerDesk executable".into());
    }
    root_file(&exe)?;
    installed_version()?;
    helper_path()?;
    if !enrolled() {
        return Err("root_enrollment_required".into());
    }
    Ok("linux-x86_64-deb")
}
pub fn helper_path() -> Result<PathBuf, String> {
    if std::env::var_os("APPIMAGE").is_some() {
        let p = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("Missing application directory")?
            .join("minerdesk-update-helper");
        if fs::symlink_metadata(&p)
            .map_err(|_| "Bundled update helper is missing")?
            .is_file()
        {
            return Ok(p);
        }
        return Err("Bundled update helper is invalid".into());
    }
    root_file(Path::new(ROOT_HELPER))?;
    Ok(ROOT_HELPER.into())
}
fn user_state() -> Result<PathBuf, String> {
    let uid = local_uid();
    let home = user(uid)?.home;
    // A deterministic location survives executable replacement and process exit.
    Ok(home.join(".local/state/minerdesk/updates"))
}
pub fn progress_path(job_id: &str) -> Result<PathBuf, String> {
    if !valid_job_id(job_id) {
        return Err("Invalid job identifier".into());
    }
    if std::env::var_os("APPIMAGE").is_some() {
        Ok(user_state()?.join("public").join(format!("{job_id}.json")))
    } else {
        Ok(Path::new(BASE)
            .join("public")
            .join(format!("{}-{job_id}.json", local_uid())))
    }
}

pub fn sudoers_entry(uid: u32) -> Result<String, String> {
    if uid == 0 {
        return Err("Root cannot enroll a remote user grant".into());
    }
    Ok(format!("# MinerDesk: only the fixed signed-update worker; no shell or arbitrary arguments.\n#{uid} ALL=(root) NOPASSWD: {ROOT_HELPER} --apply\n"))
}
fn enroll(revoke: bool) -> Result<(), String> {
    if unsafe { libc::geteuid() } != 0 || local_uid() != 0 {
        return Err("Use the local operating-system authorization prompt".into());
    }
    let uid: u32 = std::env::var("PKEXEC_UID")
        .map_err(|_| "Enrollment requires local pkexec authorization")?
        .parse()
        .map_err(|_| "Invalid pkexec caller")?;
    if uid == 0 {
        return Err("Enrollment requires an ordinary local user".into());
    }
    root_file(Path::new(ROOT_HELPER))?;
    if std::env::current_exe().map_err(|e| e.to_string())? != Path::new(ROOT_HELPER) {
        return Err("Only the packaged fixed helper may enroll updates".into());
    }
    installed_version()?;
    create_root_dir(Path::new("/var/lib/minerdesk"), 0o755)?;
    create_root_dir(Path::new(BASE), 0o755)?;
    create_root_dir(&Path::new(BASE).join("grants"), 0o700)?;
    create_root_dir(&Path::new(BASE).join("public"), 0o755)?;
    create_root_dir(&Path::new(BASE).join("private"), 0o700)?;
    let policy = PathBuf::from(format!("{SUDOERS}{uid}"));
    let public = Path::new(BASE)
        .join("public")
        .join(format!("{uid}-enrolled.json"));
    protected_path(Path::new("/etc/sudoers.d"), 0, true)?;
    if revoke {
        for p in [&policy, &grant_path(uid), &public] {
            if p.exists() {
                root_file(p)?;
                fs::remove_file(p).map_err(|e| e.to_string())?;
            }
        }
    } else {
        root_file(Path::new("/usr/sbin/visudo"))?;
        let draft = Path::new(BASE)
            .join("private")
            .join(format!("sudoers-{}", uuid::Uuid::new_v4()));
        atomic(&draft, sudoers_entry(uid)?.as_bytes(), 0o440)?;
        let valid = Command::new("/usr/sbin/visudo")
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .args(["-cf"])
            .arg(&draft)
            .output()
            .map_err(|e| e.to_string())?;
        let data = fs::read(&draft).map_err(|e| e.to_string())?;
        fs::remove_file(&draft).map_err(|e| e.to_string())?;
        if !valid.status.success() {
            return Err("The operating system rejected the narrow update policy".into());
        }
        atomic(
            &grant_path(uid),
            &serde_json::to_vec(&Grant { schema: 1, uid }).unwrap(),
            0o600,
        )?;
        atomic(&policy, &data, 0o440)?;
        atomic(&public, b"{\"enrolled\":true}", 0o644)?;
    }
    println!("{{\"enrolled\":{}}}", !revoke);
    Ok(())
}

struct User {
    uid: u32,
    gid: u32,
    name: String,
    home: PathBuf,
}
fn user(uid: u32) -> Result<User, String> {
    let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut found = std::ptr::null_mut();
    let mut buf = vec![0u8; 65536];
    if unsafe {
        libc::getpwuid_r(
            uid,
            pwd.as_mut_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut found,
        )
    } != 0
        || found.is_null()
    {
        return Err("Original update user no longer exists".into());
    }
    let pwd = unsafe { pwd.assume_init() };
    let string = |p| unsafe { std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned() };
    Ok(User {
        uid,
        gid: pwd.pw_gid,
        name: string(pwd.pw_name),
        home: string(pwd.pw_dir).into(),
    })
}
fn proc_value(pid: u32, name: &str, max: usize) -> Result<Vec<u8>, String> {
    let f =
        File::open(format!("/proc/{pid}/{name}")).map_err(|_| "Original agent is unavailable")?;
    let mut b = Vec::new();
    f.take(max as u64 + 1)
        .read_to_end(&mut b)
        .map_err(|e| e.to_string())?;
    if b.len() > max {
        return Err("Original process metadata is too large".into());
    }
    Ok(b)
}
fn parent(pid: u32) -> Result<u32, String> {
    let b = proc_value(pid, "status", 32768)?;
    let s = String::from_utf8_lossy(&b);
    s.lines()
        .find_map(|l| l.strip_prefix("PPid:"))
        .ok_or("Missing original process parent")?
        .trim()
        .parse()
        .map_err(|_| "Invalid process parent".into())
}
fn downloaded_bytes(path: &Path, uid: u32) -> Result<Vec<u8>, String> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let parent = path.parent().ok_or("Missing download directory")?;
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(parent)
        .map_err(|_| "Download directory is unavailable or a link")?;
    let m = directory.metadata().map_err(|e| e.to_string())?;
    if !m.is_dir() || m.uid() != uid || m.mode() & 0o077 != 0 {
        return Err("Download must be in the original user's private staging directory".into());
    }
    let name = std::ffi::CString::new(
        path.file_name()
            .ok_or("Missing download filename")?
            .as_encoded_bytes(),
    )
    .map_err(|_| "Invalid download filename")?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err("Download is unavailable or a link".into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.uid() != uid
        || m.nlink() != 1
        || m.mode() & 0o022 != 0
        || m.len() == 0
        || m.len() > PACKAGE_LIMIT
    {
        return Err("Download is not the original user's bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(PACKAGE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 != m.len() {
        return Err("Download changed during staging".into());
    }
    Ok(bytes)
}
fn start_tick(pid: u32) -> Result<String, String> {
    let b = proc_value(pid, "stat", 32768)?;
    let s = String::from_utf8_lossy(&b);
    s.rsplit_once(')')
        .ok_or("Invalid process identity")?
        .1
        .split_whitespace()
        .nth(19)
        .map(str::to_owned)
        .ok_or("Missing process identity".into())
}
struct Context {
    pid: u32,
    tick: String,
    user: User,
    executable: PathBuf,
    args: Vec<std::ffi::OsString>,
    env: Vec<(String, String)>,
    appimage: Option<PathBuf>,
    version: String,
}
fn relaunch_environment(bytes: &[u8], appimage: bool) -> Vec<(String, String)> {
    bytes
        .split(|b| *b == 0)
        .filter_map(|b| String::from_utf8(b.to_vec()).ok())
        .filter_map(|s| s.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())))
        .filter(|(k, _)| {
            [
                "HOME",
                "DISPLAY",
                "WAYLAND_DISPLAY",
                "XAUTHORITY",
                "XDG_RUNTIME_DIR",
                "DBUS_SESSION_BUS_ADDRESS",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "XDG_STATE_HOME",
                "LANG",
            ]
            .contains(&k.as_str())
                || (appimage && k == "APPIMAGE_EXTRACT_AND_RUN")
        })
        .collect()
}
fn context() -> Result<Context, String> {
    let root = unsafe { libc::geteuid() } == 0;
    let mut pid = unsafe { libc::getppid() } as u32;
    let uid = if root {
        if local_uid() != 0 {
            return Err("Setuid update helpers are not supported".into());
        }
        let sudo = fs::read_link(format!("/proc/{pid}/exe")).map_err(|_| "Missing sudo caller")?;
        if sudo != Path::new("/usr/bin/sudo") {
            return Err("Unattended installation must use the fixed sudo policy".into());
        }
        let uid: u32 = std::env::var("SUDO_UID")
            .map_err(|_| "Missing operating-system caller identity")?
            .parse()
            .map_err(|_| "Invalid operating-system caller identity")?;
        grant(uid)?;
        // sudo may use both a monitor and a command process. Walk only actual
        // root-owned sudo executables, then require the user's installed agent.
        for _ in 0..4 {
            pid = parent(pid)?;
            let path =
                fs::read_link(format!("/proc/{pid}/exe")).map_err(|_| "Missing original agent")?;
            if path != Path::new("/usr/bin/sudo") {
                break;
            }
            root_file(&path)?;
        }
        uid
    } else {
        local_uid()
    };
    if uid == 0 {
        return Err("Remote updates require an original ordinary user".into());
    }
    let tick = start_tick(pid)?;
    let metadata =
        fs::metadata(format!("/proc/{pid}")).map_err(|_| "Original agent is unavailable")?;
    if metadata.uid() != uid {
        return Err("Original agent belongs to another user".into());
    }
    let executable = fs::read_link(format!("/proc/{pid}/exe"))
        .map_err(|_| "Original agent executable is unavailable")?;
    let args: Vec<_> = proc_value(pid, "cmdline", 16384)?
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .skip(1)
        .map(|s| {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec(s.to_vec())
        })
        .collect();
    // These values are used only by relaunch(), whose pre_exec drops all root
    // identity before exec. Root storage and installer commands never use them.
    let env = relaunch_environment(&proc_value(pid, "environ", 65536)?, !root);
    let image = if root { None } else { Some(appimage()?) };
    let version = if root {
        if ![
            Path::new("/usr/bin/minerdesk"),
            Path::new("/usr/bin/minerdesk-headless"),
        ]
        .contains(&executable.as_path())
        {
            return Err("Only the installed MinerDesk agent can update its Debian package".into());
        }
        root_file(&executable)?;
        installed_version()?
    } else {
        if ![
            Some(std::ffi::OsStr::new("minerdesk")),
            Some(std::ffi::OsStr::new("minerdesk-headless")),
        ]
        .contains(&executable.file_name())
        {
            return Err("Original process is not a MinerDesk agent".into());
        }
        // This is an unprivileged replacement. The helper build and AppImage
        // executable are distributed together and cannot elevate the caller.
        env!("CARGO_PKG_VERSION").to_string()
    };
    if start_tick(pid)? != tick {
        return Err("Original agent changed during identity capture".into());
    }
    Ok(Context {
        pid,
        tick,
        user: user(uid)?,
        executable,
        args,
        env,
        appimage: image,
        version,
    })
}
fn stage_base(root: bool) -> Result<PathBuf, String> {
    if root {
        create_root_dir(Path::new(BASE), 0o755)?;
        create_root_dir(&Path::new(BASE).join("private"), 0o700)?;
        Ok(Path::new(BASE).join("private"))
    } else {
        let base = user_state()?;
        // Each ancestor is checked after creating it. A symlink is never
        // followed as update storage even inside the original user's home.
        let mut walk = user(local_uid())?.home;
        for c in [".local", "state", "minerdesk", "updates"] {
            walk.push(c);
            if fs::symlink_metadata(&walk).is_err() {
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&walk)
                    .map_err(|e| e.to_string())?;
            }
            protected_path(&walk, local_uid(), true)?;
        }
        Ok(base)
    }
}
fn survive_install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let root = unsafe { libc::geteuid() } == 0;
    let base = stage_base(root)?;
    if exe.starts_with(&base) {
        protected_path(&exe, unsafe { libc::geteuid() }, false)?;
        return Ok(());
    }
    if root && exe != Path::new(ROOT_HELPER) {
        return Err("Privileged worker is not the installed fixed helper".into());
    }
    if root {
        root_file(&exe)?;
    }
    let dir = base.join(format!("worker-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| e.to_string())?;
    let snapshot = dir.join("minerdesk-update-helper");
    atomic(&snapshot, &regular_bytes(&exe)?, 0o700)?;
    // Exec preserves PID, parent identity and handshake pipes while freeing the
    // installed executable and AppImage mount before either can be replaced.
    Err(Command::new(snapshot).arg("--apply").exec().to_string())
}
fn event(path: &Path, req: &Request, stage: &str, error: Option<String>) -> Result<(), String> {
    let event = WorkerEvent {
        schema: 1,
        job_id: req.job_id.clone(),
        stage: stage.into(),
        error,
    };
    let mut bytes = serde_json::to_vec(&event).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    atomic(path, &bytes, 0o644)?;
    if stage == "ready" {
        std::io::stdout()
            .write_all(&bytes)
            .and_then(|_| std::io::stdout().flush())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn relaunch(ctx: &Context) -> Result<(), String> {
    let exe = ctx.appimage.as_ref().unwrap_or(&ctx.executable);
    if ctx.appimage.is_none() {
        root_file(exe)?;
    }
    let mut command = Command::new(exe);
    command
        .args(&ctx.args)
        .env_clear()
        .env("HOME", &ctx.user.home)
        .env("USER", &ctx.user.name)
        .env("LOGNAME", &ctx.user.name)
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .envs(ctx.env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let uid = ctx.user.uid;
    let gid = ctx.user.gid;
    let name =
        std::ffi::CString::new(ctx.user.name.as_str()).map_err(|_| "Invalid original user name")?;
    if ctx.appimage.is_some()
        && ctx.executable.file_name() == Some(std::ffi::OsStr::new("minerdesk-headless"))
        && !ctx.args.iter().any(|arg| arg == "--headless")
    {
        command.arg("--headless");
    }
    unsafe {
        command.pre_exec(move || {
            libc::setsid();
            if libc::geteuid() == 0 {
                if libc::initgroups(name.as_ptr(), gid) != 0
                    || libc::setgid(gid) != 0
                    || libc::setuid(uid) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
            }
            if libc::getuid() != uid || libc::geteuid() != uid {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Cannot restore original update user",
                ));
            }
            Ok(())
        });
    }
    command
        .spawn()
        .map_err(|e| format!("Cannot restart original MinerDesk agent: {e}"))?;
    Ok(())
}
fn apply() -> Result<(), String> {
    survive_install()?;
    unsafe {
        libc::setsid();
    } // Detached from the agent's terminal/session.
    let ctx = context()?;
    let mut stdin = BufReader::new(std::io::stdin());
    let req: Request = read_message(&mut stdin)?;
    req.validate()?;
    let root = ctx.appimage.is_none();
    let base = stage_base(root)?;
    let dir = base.join(format!("job-{}", req.job_id));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|_| "Update job already exists or cannot be staged")?;
    let public_dir = if root {
        Path::new(BASE).join("public")
    } else {
        base.join("public")
    };
    if root {
        create_root_dir(&public_dir, 0o755)?;
    } else if !public_dir.exists() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&public_dir)
            .map_err(|e| e.to_string())?;
    }
    protected_path(&public_dir, unsafe { libc::geteuid() }, true)?;
    let progress = public_dir.join(if root {
        format!("{}-{}.json", ctx.user.uid, req.job_id)
    } else {
        format!("{}.json", req.job_id)
    });
    let result = (|| {
        event(&progress, &req, "verifying", None)?;
        let target = if root {
            "linux-x86_64-deb"
        } else {
            "linux-x86_64-appimage"
        };
        let artifact = official_artifact(&req.expected_version, &ctx.version, target)?;
        let bytes = downloaded_bytes(&req.package_path, ctx.user.uid)?;
        verify_package(&bytes, &artifact.signature, &req.expected_version)?;
        let package = if let Some(image) = &ctx.appimage {
            image
                .parent()
                .ok_or("Missing AppImage parent")?
                .join(format!(".minerdesk-update-{}", req.job_id))
        } else {
            dir.join("update.deb")
        };
        // create_new and a single held descriptor prevent final-component links.
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(if root { 0o600 } else { 0o700 })
            .custom_flags(libc::O_NOFOLLOW)
            .open(&package)
            .map_err(|e| e.to_string())?;
        output
            .write_all(&bytes)
            .and_then(|_| output.sync_all())
            .map_err(|e| e.to_string())?;
        drop(output);
        drop(bytes);
        if root {
            root_file(Path::new("/usr/bin/dpkg-deb"))?;
            let info = Command::new("/usr/bin/dpkg-deb")
                .env_clear()
                .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
                .args(["--field"])
                .arg(&package)
                .args(["Package", "Version"])
                .output()
                .map_err(|e| e.to_string())?;
            let want = format!("Package: miner-desk\nVersion: {}\n", req.expected_version);
            if !info.status.success() || info.stdout != want.as_bytes() {
                return Err(
                    "Signed Debian package identity/version does not match the update".into(),
                );
            }
        }
        event(&progress, &req, "ready", None)?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(read_message::<Ack>(&mut stdin));
        });
        let ack = rx
            .recv_timeout(Duration::from_secs(90))
            .map_err(|_| "Agent did not acknowledge the prepared update")??;
        if ack.schema != 1 || ack.job_id != req.job_id || !ack.apply {
            return Err("Agent cancelled the prepared update".into());
        }
        let until = Instant::now() + Duration::from_secs(120);
        while start_tick(ctx.pid).is_ok_and(|tick| tick == ctx.tick) {
            if Instant::now() >= until {
                return Err("Original agent did not exit; installation cancelled".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        finish_after_parent_exit(
            || {
                event(&progress, &req, "installing", None)?;
                if let Some(image) = &ctx.appimage {
                    let m = fs::symlink_metadata(image).map_err(|e| e.to_string())?;
                    if !m.is_file() || m.uid() != ctx.user.uid || m.nlink() != 1 {
                        return Err("AppImage target identity changed".into());
                    }
                    fs::rename(&package, image)
                        .map_err(|e| e.to_string())
                        .and_then(|_| {
                            File::open(image.parent().unwrap())
                                .and_then(|f| f.sync_all())
                                .map_err(|e| e.to_string())
                        })
                } else {
                    grant(ctx.user.uid)?; // A local revocation wins even after preparation.
                    root_file(Path::new("/usr/bin/dpkg"))?;
                    let out = Command::new("/usr/bin/dpkg")
                        .env_clear()
                        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
                        .env("HOME", "/root")
                        .arg("--install")
                        .arg(&package)
                        .output()
                        .map_err(|e| e.to_string())?;
                    if out.status.success() {
                        Ok(())
                    } else {
                        Err(format!(
                            "Debian installer failed ({:?}): {}",
                            out.status.code().or(out.status.signal()),
                            String::from_utf8_lossy(&out.stderr)
                                .chars()
                                .take(2048)
                                .collect::<String>()
                        ))
                    }
                }
            },
            || event(&progress, &req, "restarting", None),
            || relaunch(&ctx),
        )?;
        event(&progress, &req, "completed", None)
    })();
    if let Err(error) = &result {
        let _ = atomic(&dir.join("error.txt"), error.as_bytes(), 0o600);
        let _ = event(
            &progress,
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
        [mode] if mode == "--apply" => apply(),
        [mode] if mode == "--enroll" => enroll(false),
        [mode] if mode == "--revoke" => enroll(true),
        _ => Err("Expected exactly --apply, --enroll or --revoke".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn sudo_policy_has_only_the_literal_apply_argument() {
        let line = sudoers_entry(1000).unwrap();
        assert_eq!(
            line.lines().last().unwrap(),
            "#1000 ALL=(root) NOPASSWD: /usr/lib/minerdesk/minerdesk-update-helper --apply"
        );
        for forbidden in ["SETENV", "*", "/bin/sh", "--enroll", "--revoke"] {
            assert!(!line.contains(forbidden));
        }
        assert!(sudoers_entry(0).is_err());
    }
    #[test]
    fn downloaded_source_is_pinned_to_a_private_directory_descriptor() {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = temp.path().join("package");
        fs::write(&path, b"fixture only").unwrap();
        assert_eq!(
            downloaded_bytes(&path, local_uid()).unwrap(),
            b"fixture only"
        );
        assert!(downloaded_bytes(&path, local_uid().wrapping_add(1)).is_err());
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(downloaded_bytes(&link, local_uid()).is_err());
        let hard = temp.path().join("hard");
        fs::hard_link(&path, &hard).unwrap();
        assert!(downloaded_bytes(&path, local_uid()).is_err());
        fs::remove_file(hard).unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(downloaded_bytes(&path, local_uid()).is_err());
    }
    #[test]
    fn current_process_parent_and_identity_are_os_derived() {
        let pid = std::process::id();
        assert_eq!(parent(pid).unwrap(), unsafe { libc::getppid() } as u32);
        assert!(!start_tick(pid).unwrap().is_empty());
    }
    #[test]
    fn relaunch_keeps_custom_home_and_the_unprivileged_appimage_runtime_mode() {
        let bytes = b"HOME=/tmp/fixture-custom-home\0XDG_CONFIG_HOME=/tmp/fixture-config\0APPIMAGE_EXTRACT_AND_RUN=1\0LD_PRELOAD=untrusted.so\0PATH=/untrusted\0SUDO_UID=42\0";
        let image = relaunch_environment(bytes, true);
        assert!(image.contains(&("HOME".into(), "/tmp/fixture-custom-home".into())));
        assert!(image.contains(&("XDG_CONFIG_HOME".into(), "/tmp/fixture-config".into())));
        assert!(image.contains(&("APPIMAGE_EXTRACT_AND_RUN".into(), "1".into())));
        for denied in ["LD_PRELOAD", "PATH", "SUDO_UID"] {
            assert!(!image.iter().any(|(key, _)| key == denied));
        }
        let deb = relaunch_environment(bytes, false);
        assert!(deb.contains(&("HOME".into(), "/tmp/fixture-custom-home".into())));
        assert!(!deb.iter().any(|(key, _)| key == "APPIMAGE_EXTRACT_AND_RUN"));
        let custom_home_only = relaunch_environment(b"HOME=/tmp/fixture-custom-home\0", false);
        assert_eq!(
            custom_home_only,
            [("HOME".into(), "/tmp/fixture-custom-home".into())]
        );
    }
}
