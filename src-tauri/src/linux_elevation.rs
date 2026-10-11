use super::{Child, Command, Duration, Path, PathBuf, ProcessReader, Stdio};
use std::{io::{BufReader, Read, Write}, os::unix::fs::PermissionsExt, process::ChildStdin, sync::mpsc, time::Instant};

pub const AUTH_TIMEOUT: Duration = Duration::from_secs(120);
pub struct ElevatedMiner { pub child: Child, pub control: ChildStdin, pub stdout: ProcessReader, pub stderr: ProcessReader, pub miner_pid: u32 }

pub fn helper_path() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let parent = executable.parent()?;
    let mut candidates = vec![parent.join("minerdesk-linux-helper"), parent.join("resources/minerdesk-linux-helper")];
    if let Some(appdir) = std::env::var_os("APPDIR") {
        let appdir = PathBuf::from(appdir);
        candidates.push(appdir.join("usr/lib/MinerDesk/resources/minerdesk-linux-helper"));
        candidates.push(appdir.join("usr/lib/minerdesk/resources/minerdesk-linux-helper"));
        candidates.push(appdir.join("usr/lib/MinerDesk/resources/resources/minerdesk-linux-helper"));
        candidates.push(appdir.join("usr/lib/miner-desk/resources/minerdesk-linux-helper"));
        candidates.push(appdir.join("usr/bin/minerdesk-linux-helper"));
    }
    // Tauri's Linux resource directory uses the product name.
    candidates.push(PathBuf::from("/usr/lib/MinerDesk/resources/minerdesk-linux-helper"));
    candidates.push(PathBuf::from("/usr/lib/minerdesk/resources/minerdesk-linux-helper"));
    candidates.push(PathBuf::from("/usr/lib/MinerDesk/resources/resources/minerdesk-linux-helper"));
    candidates.push(PathBuf::from("/usr/lib/miner-desk/resources/minerdesk-linux-helper"));
    #[cfg(debug_assertions)]
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("linux-helper/target/debug/minerdesk-linux-helper"));
    candidates.into_iter().find(|path| path.is_file() && path.metadata().is_ok_and(|m|m.permissions().mode() & 0o111 != 0))
}
pub fn available() -> bool {
    helper_path().is_some() && (unsafe { libc::geteuid() } == 0 || Path::new("/usr/bin/pkexec").is_file())
}

fn prepare_helper(helper: &Path, appimage: bool) -> Result<(PathBuf, Option<tempfile::TempDir>), String> {
    if !appimage { return Ok((helper.to_path_buf(), None)); }
    // A user's FUSE AppImage mount may deny access even to root. Copy only the
    // bundled, minimal supervisor to a fresh private directory on the host.
    // Never relax FUSE permissions or install a persistent privileged helper.
    // Ignore TMPDIR: AppImage/WSL can point it at a FUSE or Windows filesystem
    // that is inaccessible to root or cannot enforce private Unix permissions.
    let directory = tempfile::Builder::new().prefix("minerdesk-elevation-")
        .permissions(std::fs::Permissions::from_mode(0o700)).tempdir_in("/tmp").map_err(|e|e.to_string())?;
    let staged = directory.path().join("minerdesk-linux-helper");
    std::fs::copy(helper, &staged).map_err(|e|e.to_string())?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o700)).map_err(|e|e.to_string())?;
    if staged.metadata().map_err(|e|e.to_string())?.permissions().mode() & 0o777 != 0o700
        || directory.path().metadata().map_err(|e|e.to_string())?.permissions().mode() & 0o777 != 0o700 {
        return Err("The host temporary filesystem cannot enforce private Linux helper permissions".into());
    }
    Ok((staged, Some(directory)))
}
fn message(code: Option<i32>, detail: &str, french: bool) -> String {
    let text = match (french,code) {
        (true,Some(126)) => "Autorisation Linux annulée : le mineur n’a pas démarré.",
        (false,Some(126)) => "Linux administrator approval was cancelled; the miner did not start.",
        (true,Some(127)) => "Autorisation administrateur Linux indisponible ou refusée. Lancez MinerDesk depuis le menu Applications et vérifiez l’agent d’authentification graphique (polkit).",
        (false,Some(127)) => "Linux administrator approval is unavailable or denied. Launch MinerDesk from the Applications menu and check the graphical polkit authentication agent.",
        (true,_) => "Le lancement du mineur en root a échoué. Vérifiez le paquet MinerDesk et la console.",
        (false,_) => "Elevated miner launch failed. Check the MinerDesk package and console.",
    };
    format!("{text} {}", detail.trim().chars().take(2048).collect::<String>())
}

pub fn spawn(path: &Path, args: &[String], french: bool) -> Result<ElevatedMiner, String> {
    let helper = helper_path().ok_or_else(|| if french { "Lanceur Linux privilégié introuvable. Réinstallez le paquet MinerDesk récent." } else { "Linux elevation helper is missing. Reinstall the current MinerDesk package." }.to_string())?;
    // Retain the private copy until the actual miner acknowledges startup. A
    // cancelled/expired approval removes it as well as closing the owner pipe.
    let (helper, _helper_directory) = prepare_helper(&helper, std::env::var_os("APPDIR").is_some())?;
    let root = unsafe { libc::geteuid() } == 0;
    if !root && !Path::new("/usr/bin/pkexec").is_file() {
        return Err(if french { "pkexec est absent. Installez pkexec et un agent polkit graphique pour autoriser le mineur." } else { "pkexec is missing. Install pkexec and a graphical polkit agent to authorize the miner." }.into());
    }
    let path = path.canonicalize().map_err(|e|e.to_string())?;
    let mut command = if root { Command::new(&helper) } else {
        let mut command = Command::new("/usr/bin/pkexec");
        command.arg("--disable-internal-agent").arg(&helper); command
    };
    command.args(["--application-version",env!("CARGO_PKG_VERSION"),"--miner"]).arg(&path).arg("--").args(args);
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e|message(None,&e.to_string(),french))?;
    let control = child.stdin.take().ok_or("Linux control pipe is unavailable")?;
    let stdout = child.stdout.take().ok_or("Linux helper output is unavailable")?;
    let mut stderr = child.stderr.take().ok_or("Linux helper errors are unavailable")?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout); let mut line = Vec::new();
        let mut byte = [0u8;1];
        while line.len() < 4096 {
            match reader.read(&mut byte) { Ok(0) | Err(_) => break, Ok(_) => { line.push(byte[0]); if byte[0]==b'\n' { break; } } }
        }
        let _ = sender.send((String::from_utf8_lossy(&line).into_owned(), reader));
    });
    match receiver.recv_timeout(AUTH_TIMEOUT) {
        Ok((line, reader)) => {
            if let Some(pid) = line.trim().strip_prefix("MINERDESK_ELEVATION_READY ").and_then(|value|value.parse::<u32>().ok()).filter(|pid|*pid>0) {
                return Ok(ElevatedMiner { child, control, stdout:Box::new(reader), stderr:Box::new(stderr), miner_pid:pid });
            }
            drop(control);
            // Preserve pkexec's cancellation/denial exit status before killing
            // a pending launcher. The closed pipe also cancels delayed approval.
            let status = cancel_launcher(&mut child);
            let mut detail = line;
            if status.is_some() { let mut bytes=Vec::new(); let _=stderr.by_ref().take(4096).read_to_end(&mut bytes); detail.push_str(&String::from_utf8_lossy(&bytes)); }
            Err(message(status.and_then(|s|s.code()),&detail,french))
        }
        Err(_) => {
            // Delayed approval cannot launch a miner: the helper rejects EOF.
            drop(control); let _=cancel_launcher(&mut child);
            Err(if french { "Délai d’autorisation Linux dépassé (120 s). Le démarrage a été annulé ; réessayez et validez la boîte de dialogue du système." } else { "Linux approval timed out after 120 seconds. The start was cancelled; retry and approve the system dialog." }.into())
        }
    }
}

fn cancel_launcher(child: &mut Child) -> Option<std::process::ExitStatus> {
    let began = Instant::now();
    let mut killed = false;
    while began.elapsed() < Duration::from_secs(6) {
        if let Ok(Some(status)) = child.try_wait() { return Some(status); }
        if !killed && began.elapsed() > Duration::from_secs(3) {
            let _ = child.kill(); killed = true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

pub fn stop(child: &mut Child, control: &mut Option<ChildStdin>) -> Result<(),String> {
    if let Some(mut pipe) = control.take() { let _=pipe.write_all(b"stop\n"); } // Drop sends EOF even if writing fails.
    let deadline=Instant::now()+Duration::from_secs(6);
    while Instant::now()<deadline {
        match child.try_wait() { Ok(Some(_))=>return Ok(()), Err(e)=>return Err(e.to_string()), _=>std::thread::sleep(Duration::from_millis(50)) }
    }
    Err("The elevated miner did not confirm shutdown. Its supervisor is still tracked; retry Stop and inspect the console.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn appimage_helper_copy_is_private_exact_and_removed_after_use() {
        let source = tempfile::tempdir().unwrap();
        let executable = source.path().join("bundled-helper");
        std::fs::write(&executable, b"fixture executable bytes").unwrap();
        let (direct, no_copy) = prepare_helper(&executable, false).unwrap();
        assert_eq!(direct, executable); assert!(no_copy.is_none());
        let (staged, directory) = prepare_helper(&executable, true).unwrap();
        assert_ne!(staged, executable);
        assert_eq!(std::fs::read(&staged).unwrap(), std::fs::read(&executable).unwrap());
        assert_eq!(staged.metadata().unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(directory.as_ref().unwrap().path().metadata().unwrap().permissions().mode() & 0o777, 0o700);
        drop(directory); assert!(!staged.exists()); assert!(executable.exists());
    }
    #[test] fn older_profiles_never_enable_root_and_explicit_choice_survives_reload() {
        let mut profile: super::super::MinerProfile = serde_json::from_str(r#"{"id":"existing","engine":"srbminer"}"#).unwrap();
        assert!(!profile.linux_run_as_root);
        profile.linux_run_as_root = true;
        let restored: super::super::MinerProfile = serde_json::from_str(&serde_json::to_string(&profile).unwrap()).unwrap();
        assert!(restored.linux_run_as_root);
        assert_eq!(restored.id, "existing");
    }
    #[test] fn approval_errors_distinguish_cancel_denial_and_helper_failures() {
        assert!(message(Some(126),"",false).contains("cancelled"));
        assert!(message(Some(127),"No authentication agent",true).contains("menu Applications"));
        assert!(message(Some(125),"wrong helper version",false).contains("wrong helper version"));
        assert!(message(Some(127),&"x".repeat(20000),false).len()<2300);
    }
}
