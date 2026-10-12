#[path = "linux_authorization_protocol.rs"]
pub mod protocol;
use super::{
    apply_dev_tip_wallets, build_engine_args_with_language, linux_elevation, mining_diagnostics,
    MinerProfile,
};
use protocol::{Enrollment, GrantRef};
use serde::Serialize;
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};
const RUNNER: &str = "/usr/lib/minerdesk/minerdesk-approved-helper";
pub static OPERATION: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub fn needed(french: bool) -> String {
    if french {"Autorisation root à renouveler : ouvrez le profil sur ce PC Linux et cliquez sur « Autoriser les démarrages planifiés ». Aucun mot de passe ne sera demandé par le planning."}else{"Root authorization required: open this profile on the Linux PC and click Authorize scheduled starts. The scheduler will not ask for a password."}.into()
}

#[derive(Serialize)]
pub struct AuthorizationStatus {
    pub state: String,
    pub eligible: bool,
    pub message: String,
    pub commands: Vec<String>,
    pub source_directory: String,
    pub fingerprint: Option<String>,
}
pub fn enrollment(profile: &MinerProfile, french: bool) -> Result<Enrollment, String> {
    if !profile.extra_args.trim().is_empty() {
        return Err(if french{"Le mode root sans surveillance exige que « Arguments avancés » soit vide. Utilisez les champs du profil ou le mode root manuel."}else{"Unattended root mode requires empty Advanced arguments. Use the profile fields or interactive root mode."}.into());
    }
    let path = Path::new(profile.executable_path.trim())
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let user_args = build_engine_args_with_language(profile, french)?;
    let support_args = if profile.dev_tip_percent > 0.0 {
        let mut tip = profile.clone();
        apply_dev_tip_wallets(&mut tip).ok_or("Voluntary-support network is unavailable")?;
        Some(build_engine_args_with_language(&tip, french)?)
    } else {
        None
    };
    let request = Enrollment {
        profile_id: profile.id.clone(),
        profile_name: profile.name.clone(),
        engine: profile.engine.clone(),
        executable: path,
        user_args,
        support_args,
        support_percent: profile.dev_tip_percent,
    };
    protocol::validate_request(&request)?;
    Ok(request)
}
fn preflight() -> Result<(), String> {
    let path = Path::new(RUNNER);
    let meta = std::fs::symlink_metadata(path).map_err(|_| {
        "The approved Linux helper is not installed; authorize the profile first".to_string()
    })?;
    if !meta.is_file() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
        return Err("The approved Linux helper has unsafe ownership or permissions".into());
    }
    if unsafe { libc::geteuid() } == 0 {
        return Ok(());
    }
    // No --allow-user-interaction: a missing/changed policy returns an error
    // before pkexec can request a password during a scheduled start.
    let result = bounded_output(
        Command::new("/usr/bin/pkcheck").args([
            "--action-id",
            "com.minerdesk.run-approved-miner",
            "--process",
            &std::process::id().to_string(),
        ]),
        Duration::from_secs(5),
    )?;
    if !result.status.success() {
        return Err(
            "System authorization is unavailable; authorize the profile again from the desktop"
                .into(),
        );
    }
    Ok(())
}
fn approved_command(grant: &GrantRef, expected: &str, mode: &str) -> Result<Command, String> {
    if !protocol::valid_hex(&grant.id, 32) || !protocol::valid_hex(expected, 64) {
        return Err("Invalid root authorization reference".into());
    }
    preflight()?;
    let mut command = if unsafe { libc::geteuid() } == 0 {
        Command::new(RUNNER)
    } else {
        let mut c = Command::new("/usr/bin/pkexec");
        c.arg("--disable-internal-agent").arg(RUNNER);
        c
    };
    command.args([
        "--application-version",
        env!("CARGO_PKG_VERSION"),
        "--grant",
        &grant.id,
        "--fingerprint",
        expected,
        "--mode",
        mode,
    ]);
    Ok(command)
}
fn bounded_output(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return child.wait_with_output().map_err(|e| e.to_string());
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            return Err("Root authorization check timed out".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
pub fn status(profile: &MinerProfile, french: bool) -> AuthorizationStatus {
    let ineligible = |message| AuthorizationStatus {
        state: "required".into(),
        eligible: false,
        message,
        commands: vec![],
        source_directory: String::new(),
        fingerprint: None,
    };
    let request = match enrollment(profile, french) {
        Ok(r) => r,
        Err(e) => return ineligible(e),
    };
    let hash = match protocol::request_fingerprint(&request) {
        Ok(h) => h,
        Err(e) => return ineligible(e),
    };
    let commands = std::iter::once(&request.user_args)
        .chain(request.support_args.iter())
        .map(|a| {
            mining_diagnostics::display_command(&request.executable.to_string_lossy(), a, false)
        })
        .collect();
    let approved = profile
        .linux_root_authorization
        .as_ref()
        .and_then(|grant| approved_command(grant, &hash, "status").ok())
        .and_then(|mut c| bounded_output(&mut c, Duration::from_secs(15)).ok())
        .is_some_and(|r| r.status.success());
    AuthorizationStatus {
        state: if approved { "approved" } else { "required" }.into(),
        eligible: true,
        message: if approved {
            if french{"Démarrages planifiés autorisés pour ce compte et ces commandes. Aucun mot de passe administrateur enregistré."}else{"Scheduled starts are approved for this account and these commands. No administrator password is stored."}.into()
        } else {
            needed(french)
        },
        commands,
        source_directory: request
            .executable
            .parent()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        fingerprint: Some(hash),
    }
}
pub fn authorize(profile: &MinerProfile, reviewed: &str, french: bool) -> Result<GrantRef, String> {
    let request = enrollment(profile, french)?;
    let fingerprint = protocol::request_fingerprint(&request)?;
    if fingerprint != reviewed {
        return Err(
            "The miner files or settings changed since the review; review the commands again"
                .into(),
        );
    }
    let original = linux_elevation::helper_path().ok_or("Linux elevation helper is missing")?;
    let (helper, _private_copy) =
        linux_elevation::prepare_helper(&original, std::env::var_os("APPDIR").is_some())?;
    if !helper
        .parent()
        .unwrap()
        .join("minerdesk-approved-helper")
        .is_file()
    {
        return Err(
            "The packaged approved helper is missing. Reinstall the current Linux package.".into(),
        );
    }
    let mut command = if unsafe { libc::geteuid() } == 0 {
        Command::new(&helper)
    } else {
        let mut c = Command::new("/usr/bin/pkexec");
        c.arg("--disable-internal-agent").arg(&helper);
        c
    };
    command.args([
        "--application-version",
        env!("CARGO_PKG_VERSION"),
        "--authorize",
    ]);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut owner = child
        .stdin
        .take()
        .ok_or("Authorization owner pipe is missing")?;
    let payload =
        serde_json::to_string(&serde_json::json!({"request":request,"fingerprint":fingerprint}))
            .map_err(|e| e.to_string())?;
    if let Err(error) = writeln!(owner, "{payload}") {
        drop(owner);
        let _ = linux_elevation::cancel_launcher(&mut child);
        return Err(error.to_string());
    }
    let stdout = child
        .stdout
        .take()
        .ok_or("Authorization output is missing")?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or("Authorization diagnostics are missing")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).take(4096).read_line(&mut line);
        let _ = tx.send(result.map(|_| line));
    });
    match rx.recv_timeout(linux_elevation::AUTH_TIMEOUT) {
        Ok(Ok(line)) => {
            if let Some(json) = line.trim().strip_prefix("MINERDESK_AUTHORIZATION ") {
                let grant: GrantRef = serde_json::from_str(json).map_err(|e| e.to_string())?;
                drop(owner);
                let _ = linux_elevation::cancel_launcher(&mut child);
                if !protocol::valid_hex(&grant.id, 32) || grant.fingerprint != fingerprint {
                    return Err("Authorization response differs from the requested profile".into());
                }
                return Ok(grant);
            }
            drop(owner);
            let code = linux_elevation::cancel_launcher(&mut child);
            let mut detail = String::new();
            if code.is_some() {
                let _ = stderr.by_ref().take(4096).read_to_string(&mut detail);
            }
            Err(format!(
                "{} {}",
                if french {
                    "Autorisation non enregistrée : validation annulée/refusée, ou dossier du mineur non admissible."
                } else {
                    "Authorization was not saved: approval cancelled/denied or the miner folder is ineligible."
                },
                detail.trim()
            ))
        }
        _ => {
            drop(owner);
            let _ = linux_elevation::cancel_launcher(&mut child);
            Err(if french{"Autorisation annulée après 120 s. Réessayez depuis le menu Applications et validez la fenêtre Linux."}else{"Authorization cancelled after 120 seconds. Retry from the desktop Applications menu and approve the Linux dialog."}.into())
        }
    }
}
pub fn revoke(grant: &GrantRef) -> Result<(), String> {
    let mut c = approved_command(grant, &grant.fingerprint, "revoke")?;
    let result = bounded_output(&mut c, Duration::from_secs(15))?;
    if result.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&result.stderr)
            .chars()
            .take(2048)
            .collect())
    }
}
pub fn spawn(
    profile: &MinerProfile,
    support: bool,
    french: bool,
) -> Result<linux_elevation::ElevatedMiner, String> {
    let grant = profile
        .linux_root_authorization
        .as_ref()
        .ok_or_else(|| needed(french))?;
    let request = enrollment(profile, french)?;
    let expected = protocol::request_fingerprint(&request)?;
    if expected != grant.fingerprint {
        return Err(needed(french));
    }
    let command = approved_command(grant, &expected, if support { "support" } else { "user" })
        .map_err(|e| format!("{} {e}", needed(french)))?;
    linux_elevation::spawn_command(command, Duration::from_secs(15), french)
        .map_err(|e| format!("{} {e}", needed(french)))
}
