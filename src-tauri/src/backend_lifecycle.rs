//! Desktop leases and the bounded, acknowledged local shutdown handshake.
//! A missing heartbeat is not proof that a Desktop process has exited.
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct DesktopLease {
    pub(crate) last_heartbeat: Instant,
    pub(crate) warned_stale: bool,
}

#[derive(Debug, Default)]
pub(crate) struct DesktopLeaseState {
    pub(crate) ever_attached: bool,
    pub(crate) clients: HashMap<u32, DesktopLease>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LeaseSnapshot {
    pub(crate) pid: u32,
    heartbeat: Instant,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LeaseProbe {
    pub(crate) lease: LeaseSnapshot,
    pub(crate) alive: Option<bool>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LeaseUpdate {
    Changed,
    Removed,
    AliveWarning,
    UnknownWarning,
    Retained,
}

impl DesktopLeaseState {
    pub(crate) fn other_clients(&self, caller: u32) -> Vec<LeaseSnapshot> {
        self.clients.iter().filter_map(|(&pid, lease)| {
            (pid != caller).then_some(LeaseSnapshot { pid, heartbeat: lease.last_heartbeat })
        }).collect()
    }

    pub(crate) fn stale_clients(&self, now: Instant, grace: Duration) -> Vec<LeaseSnapshot> {
        self.clients.iter().filter_map(|(&pid, lease)| {
            (now.saturating_duration_since(lease.last_heartbeat) >= grace)
                .then_some(LeaseSnapshot { pid, heartbeat: lease.last_heartbeat })
        }).collect()
    }

    fn unchanged(&self, snapshot: LeaseSnapshot) -> bool {
        self.clients.get(&snapshot.pid)
            .is_some_and(|lease| lease.last_heartbeat == snapshot.heartbeat)
    }

    /// Apply only to the exact heartbeat generation that was probed, after the
    /// liveness check has completed without holding the lease mutex.
    pub(crate) fn reconcile_stale(&mut self, probe: LeaseProbe, now: Instant) -> LeaseUpdate {
        if !self.unchanged(probe.lease) { return LeaseUpdate::Changed; }
        if probe.alive == Some(false) {
            self.clients.remove(&probe.lease.pid);
            return LeaseUpdate::Removed;
        }
        let lease = self.clients.get_mut(&probe.lease.pid).expect("unchanged lease");
        let warning = if lease.warned_stale { LeaseUpdate::Retained }
            else if probe.alive == Some(true) { LeaseUpdate::AliveWarning }
            else { LeaseUpdate::UnknownWarning };
        lease.warned_stale = true;
        // Throttle failed/stale process probes, not Desktop heartbeats.
        lease.last_heartbeat = now;
        warning
    }

    /// A refused shutdown must retain the caller's ownership as well. Only a
    /// confirmed dead process with an unchanged heartbeat may be discarded.
    pub(crate) fn prepare_shutdown(&mut self, caller: u32, probes: &[LeaseProbe]) -> Result<(), Vec<u32>> {
        for probe in probes {
            if probe.lease.pid != caller && probe.alive == Some(false) && self.unchanged(probe.lease) {
                self.clients.remove(&probe.lease.pid);
            }
        }
        let mut others: Vec<_> = self.clients.keys().copied().filter(|pid| *pid != caller).collect();
        if !others.is_empty() {
            others.sort_unstable();
            return Err(others);
        }
        self.clients.remove(&caller);
        Ok(())
    }

    pub(crate) fn should_stop_automatically(&self, fleet_enabled: bool) -> bool {
        !fleet_enabled && self.ever_attached && self.clients.is_empty()
    }
}

/// Native, non-waiting Windows liveness query. Lack of access is uncertainty,
/// never permission to stop somebody else's session. No process is modified.
#[cfg(target_os = "windows")]
pub(crate) fn process_is_alive(pid: u32) -> Option<bool> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    if pid == 0 { return None; }
    // SYNCHRONIZE is enough to query the process object's signalled state.
    let handle = unsafe { OpenProcess(0x0010_0000, 0, pid) };
    if handle.is_null() {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(87) => Some(false), // ERROR_INVALID_PARAMETER: no such nonzero PID.
            _ => None,
        };
    }
    let state = unsafe { WaitForSingleObject(handle, 0) };
    unsafe { CloseHandle(handle); }
    match state {
        0 => Some(false), // WAIT_OBJECT_0: the process has terminated.
        258 => Some(true), // WAIT_TIMEOUT: the process is still running.
        _ => None,
    }
}

const MAX_HEADER_BYTES: usize = 8192;
const MAX_BODY_BYTES: usize = 8192;

fn acknowledgement(body: &[u8], detach: bool) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| "The backend returned an invalid shutdown acknowledgement".to_string())?;
    if value.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err("The backend did not accept the shutdown request".into());
    }
    match value.get("ignored") {
        None | Some(serde_json::Value::Bool(false)) => Ok(()),
        Some(serde_json::Value::Bool(true)) => {
            let reason = value.get("reason").and_then(serde_json::Value::as_str).unwrap_or("");
            match reason {
                "Fleet agent remains active" if detach => Ok(()),
                "other desktop instances remain" => Err("Close the other MinerDesk Desktop windows on this PC, then retry the update. The backend is still in use by another Desktop instance.".into()),
                "standalone headless backend" => Err("This backend was started independently in command-line/headless mode. Stop that instance manually before updating MinerDesk; it will not be terminated automatically.".into()),
                "desktop-owned backend" => Err("This server is hosted by another MinerDesk Desktop process. Close that instance before updating.".into()),
                "Fleet agent remains active" => Err("The Fleet backend kept running instead of accepting the update shutdown. Quit other MinerDesk windows and retry.".into()),
                _ => Err("The backend declined shutdown. Open backend diagnostics and close other MinerDesk Desktop instances before retrying.".into()),
            }
        }
        _ => Err("The backend returned an invalid shutdown acknowledgement".into()),
    }
}

/// Returns header length and an optional fixed body length. Connection-close
/// framing is accepted too. This talks directly to the local Axum endpoint,
/// which emits a fixed JSON body; arbitrary transfer encodings are rejected.
fn response_header(bytes: &[u8]) -> Result<Option<(usize, Option<usize>)>, String> {
    let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
        if bytes.len() > MAX_HEADER_BYTES { return Err("Backend shutdown response headers are too large".into()); }
        return Ok(None);
    };
    if end + 4 > MAX_HEADER_BYTES { return Err("Backend shutdown response headers are too large".into()); }
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| "Invalid backend shutdown HTTP response")?;
    let mut lines = header.split("\r\n");
    let status = lines.next().unwrap_or("");
    if status != "HTTP/1.1 200 OK" && status != "HTTP/1.0 200 OK" {
        return Err("The backend refused the shutdown request (unexpected HTTP status). Open backend diagnostics before retrying.".into());
    }
    let mut length = None;
    let mut json = false;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("Invalid backend shutdown HTTP headers")?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() { return Err("Duplicate backend shutdown response length".into()); }
            let parsed = value.parse::<usize>().map_err(|_| "Invalid backend shutdown response length")?;
            if parsed > MAX_BODY_BYTES { return Err("Backend shutdown response is too large".into()); }
            length = Some(parsed);
        }
        if name.eq_ignore_ascii_case("content-type") {
            json = value.split(';').next().is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"));
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err("Unsupported backend shutdown response encoding".into());
        }
    }
    if !json { return Err("The backend did not return a JSON shutdown acknowledgement".into()); }
    Ok(Some((end + 4, length)))
}

fn read_acknowledgement(stream: &mut TcpStream, detach: bool, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut response = Vec::new();
    loop {
        if let Some((start, length)) = response_header(&response)? {
            if let Some(length) = length {
                if response.len() >= start + length {
                    return acknowledgement(&response[start..start + length], detach);
                }
            } else if response.len() - start > MAX_BODY_BYTES {
                return Err("Backend shutdown response is too large".into());
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { return Err("Timed out waiting for the backend shutdown acknowledgement; installation was not started".into()); }
        stream.set_read_timeout(Some(remaining)).map_err(|e| format!("Cannot bound backend shutdown response: {e}"))?;
        let mut chunk = [0u8; 2048];
        let n = stream.read(&mut chunk).map_err(|e| format!("Cannot read backend shutdown acknowledgement: {e}"))?;
        if n == 0 {
            let Some((start, length)) = response_header(&response)? else {
                return Err("The backend closed without acknowledging shutdown".into());
            };
            if length.is_some_and(|length| response.len() - start != length) {
                return Err("The backend returned an incomplete shutdown acknowledgement".into());
            }
            return acknowledgement(&response[start..], detach);
        }
        response.extend_from_slice(&chunk[..n]);
        if response.len() > MAX_HEADER_BYTES + MAX_BODY_BYTES {
            return Err("Backend shutdown response is too large".into());
        }
    }
}

pub(crate) fn request_shutdown(port: u16, pid: u32, detach: bool) -> Result<(), String> {
    request_shutdown_with_budget(port, pid, detach, MAINTENANCE_BUDGET)
}

#[derive(Clone, Copy)]
struct ShutdownBudget { connect: Duration, write: Duration, ack: Duration }
const MAINTENANCE_BUDGET: ShutdownBudget = ShutdownBudget {
    connect: Duration::from_millis(800), write: Duration::from_millis(800), ack: Duration::from_secs(30),
};
const EXIT_BUDGET: ShutdownBudget = ShutdownBudget {
    connect: Duration::from_millis(200), write: Duration::from_millis(100), ack: Duration::from_millis(400),
};

/// Window close and tray Quit are best effort and must never inherit the long
/// maintenance acknowledgement timeout on the native UI thread.
pub(crate) fn request_shutdown_for_exit(port: u16, pid: u32, detach: bool) -> Result<(), String> {
    request_shutdown_with_budget(port, pid, detach, EXIT_BUDGET)
}

#[cfg(test)]
fn request_shutdown_bounded(port: u16, pid: u32, detach: bool, ack_timeout: Duration) -> Result<(), String> {
    request_shutdown_with_budget(port, pid, detach, ShutdownBudget { ack: ack_timeout, ..MAINTENANCE_BUDGET })
}

fn request_shutdown_with_budget(port: u16, pid: u32, detach: bool, budget: ShutdownBudget) -> Result<(), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, budget.connect)
        .map_err(|e| format!("Cannot connect to the backend for shutdown: {e}"))?;
    stream.set_write_timeout(Some(budget.write))
        .map_err(|e| format!("Cannot bound the backend shutdown request: {e}"))?;
    let detach_header = if detach { "X-MinerDesk-Desktop-Detach: 1\r\n" } else { "" };
    let request = format!("POST /api/backend/shutdown HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-MinerDesk-Desktop-Pid: {pid}\r\n{detach_header}Accept: application/json\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|e| format!("Cannot send backend shutdown request: {e}"))?;
    read_acknowledgement(&mut stream, detach, budget.ack)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(pids: &[u32]) -> DesktopLeaseState {
        let old = Instant::now() - Duration::from_secs(30);
        DesktopLeaseState { ever_attached: true, clients: pids.iter().map(|&pid| (pid, DesktopLease { last_heartbeat: old, warned_stale: false })).collect() }
    }
    #[test]
    fn stale_fleet_lease_is_pruned_without_stopping_the_autonomous_agent() {
        let mut leases = state(&[10]);
        let stale = leases.stale_clients(Instant::now(), Duration::from_secs(15));
        assert_eq!(stale.len(), 1);
        assert_eq!(leases.reconcile_stale(LeaseProbe { lease: stale[0], alive: Some(false) }, Instant::now()), LeaseUpdate::Removed);
        assert!(leases.clients.is_empty());
        assert!(!leases.should_stop_automatically(true));
        assert!(leases.should_stop_automatically(false));
        assert!(!DesktopLeaseState::default().should_stop_automatically(false));
    }
    #[test]
    fn explicit_shutdown_prunes_dead_peers_but_preserves_live_unknown_and_caller() {
        let mut leases = state(&[10, 20, 30, 40]);
        let probes: Vec<_> = leases.other_clients(10).into_iter().map(|lease| LeaseProbe { alive: match lease.pid { 20 => Some(false), 30 => Some(true), _ => None }, lease }).collect();
        assert_eq!(leases.prepare_shutdown(10, &probes), Err(vec![30, 40]));
        assert!(leases.clients.contains_key(&10));
        assert!(!leases.clients.contains_key(&20));
        assert!(leases.clients.contains_key(&30) && leases.clients.contains_key(&40));
        let probes: Vec<_> = leases.other_clients(10).into_iter().map(|lease| LeaseProbe { lease, alive: Some(false) }).collect();
        assert_eq!(leases.prepare_shutdown(10, &probes), Ok(()));
        assert!(leases.clients.is_empty());
    }
    #[test]
    fn heartbeat_arriving_during_a_dead_pid_probe_preserves_that_lease() {
        let mut leases = state(&[10, 20]);
        let snapshot = leases.other_clients(10)[0];
        leases.clients.get_mut(&20).unwrap().last_heartbeat = Instant::now();
        let probe = LeaseProbe { lease: snapshot, alive: Some(false) };
        assert_eq!(leases.reconcile_stale(probe, Instant::now()), LeaseUpdate::Changed);
        assert_eq!(leases.prepare_shutdown(10, &[probe]), Err(vec![20]));
        assert!(leases.clients.contains_key(&10) && leases.clients.contains_key(&20));
    }
    #[test]
    fn newly_attached_peer_after_snapshot_also_blocks_shutdown() {
        let mut leases = state(&[10, 20]);
        let probe = LeaseProbe { lease: leases.other_clients(10)[0], alive: Some(false) };
        leases.clients.insert(30, DesktopLease { last_heartbeat: Instant::now(), warned_stale: false });
        assert_eq!(leases.prepare_shutdown(10, &[probe]), Err(vec![30]));
        assert!(leases.clients.contains_key(&10));
    }
    #[test]
    fn live_or_unverifiable_stale_clients_are_retained_and_probe_is_throttled() {
        for alive in [Some(true), None] {
            let mut leases = state(&[10]);
            let probe = LeaseProbe { lease: leases.stale_clients(Instant::now(), Duration::from_secs(15))[0], alive };
            assert!(matches!(leases.reconcile_stale(probe, Instant::now()), LeaseUpdate::AliveWarning | LeaseUpdate::UnknownWarning));
            assert_eq!(leases.clients.len(), 1);
            assert!(leases.stale_clients(Instant::now(), Duration::from_secs(15)).is_empty());
            assert!(!leases.should_stop_automatically(false));
        }
    }
    fn response(body: &str) -> Vec<u8> {
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
    }
    fn wire(body: Vec<u8>, detach: bool) -> Result<(), String> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut request = Vec::new(); let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") { assert_eq!(stream.read(&mut byte).unwrap(), 1); request.push(byte[0]); }
            let text = String::from_utf8(request).unwrap();
            assert!(text.starts_with("POST /api/backend/shutdown HTTP/1.1\r\n"));
            assert!(text.contains("X-MinerDesk-Desktop-Pid: 42\r\n"));
            assert_eq!(text.contains("X-MinerDesk-Desktop-Detach: 1\r\n"), detach);
            // Fragmented wire frames exercise the bounded response reader.
            for part in body.chunks(7) { if stream.write_all(part).is_err() { break; } }
        });
        let result = request_shutdown_bounded(port, 42, detach, Duration::from_secs(2));
        peer.join().unwrap();
        result
    }
    #[test]
    fn wire_accepts_only_a_real_shutdown_or_an_explicit_fleet_detach() {
        assert!(wire(response(r#"{"ok":true,"pid":123}"#), false).is_ok());
        let fleet = response(r#"{"ok":true,"ignored":true,"reason":"Fleet agent remains active"}"#);
        assert!(wire(fleet.clone(), true).is_ok());
        assert!(wire(fleet, false).unwrap_err().contains("kept running"));
        for (reason, message) in [("other desktop instances remain", "Close the other"), ("standalone headless backend", "independently"), ("desktop-owned backend", "another MinerDesk Desktop")] {
            let body = format!(r#"{{"ok":true,"ignored":true,"reason":"{reason}"}}"#);
            for detach in [false, true] { assert!(wire(response(&body), detach).unwrap_err().contains(message)); }
        }
    }
    #[test]
    fn wire_fails_closed_on_auth_failure_malformed_or_truncated_acknowledgements() {
        for body in [r#"{"ok":false}"#, r#"{"ok":true,"ignored":"false"}"#, r#"{"ok":true,"ignored":true}"#, "{}", "not JSON"] {
            assert!(wire(response(body), false).is_err());
        }
        assert!(wire(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n".to_vec(), false).is_err());
        assert!(wire(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 99\r\n\r\n{\"ok\":true}".to_vec(), false).is_err());
        assert!(wire(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\n\r\n{}".to_vec(), false).is_err());
        assert!(wire(Vec::new(), false).is_err());
    }
    #[test]
    fn response_limits_and_body_framing_are_enforced() {
        assert!(response_header(&vec![b'a'; MAX_HEADER_BYTES + 1]).is_err());
        assert!(response_header(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 999999\r\n\r\n").is_err());
        assert!(response_header(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}").is_err());
        assert!(response_header(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n").is_err());
        assert!(wire(b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}".to_vec(), false).is_ok());
    }
    #[test]
    fn silent_peer_has_a_bounded_acknowledgement_deadline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || { let (_stream, _) = listener.accept().unwrap(); std::thread::sleep(Duration::from_millis(250)); });
        let started = Instant::now();
        assert!(request_shutdown_bounded(port, 42, false, Duration::from_millis(40)).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        peer.join().unwrap();
    }
    #[test]
    fn native_exit_profile_stays_short_while_maintenance_can_wait_for_legacy_shutdown() {
        assert!(EXIT_BUDGET.connect + EXIT_BUDGET.write + EXIT_BUDGET.ack < Duration::from_secs(1));
        assert_eq!(MAINTENANCE_BUDGET.ack, Duration::from_secs(30));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 512]; let _ = stream.read(&mut request);
            std::thread::sleep(Duration::from_millis(900));
        });
        let started = Instant::now();
        assert!(request_shutdown_for_exit(port, 42, false).is_err());
        assert!(started.elapsed() < Duration::from_millis(800));
        peer.join().unwrap();
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn native_liveness_query_keeps_current_process_and_handles_uncertainty() {
        assert_eq!(process_is_alive(std::process::id()), Some(true));
        assert_eq!(process_is_alive(0), None);
        assert_eq!(process_is_alive(u32::MAX), Some(false));
    }
}
