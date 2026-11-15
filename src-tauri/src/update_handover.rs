//! Fail closed while the Windows updater hands the managed backend to setup.
//! An HTTP health timeout is not evidence that the executable has stopped.
use std::{io::ErrorKind, net::{SocketAddr, TcpStream}, time::Duration};

pub fn listener_stopped(port: u16) -> Result<bool, String> {
    match TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(300)) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == ErrorKind::ConnectionRefused => Ok(true),
        Err(error) => Err(format!("Could not verify that the Windows backend stopped: {error}. Installation was not started.")),
    }
}

pub fn backend_stopped(port: u16, pid: Option<u32>, process_alive: impl FnOnce(u32) -> Option<bool>) -> Result<bool, String> {
    if !listener_stopped(port)? { return Ok(false); }
    // Older backends did not report a PID. Setup retains its own running-process
    // preflight; never infer shutdown from an unhealthy but still-open listener.
    match pid {
        Some(pid) => match process_alive(pid) {
            Some(alive) => Ok(!alive),
            None => Err("Windows could not verify that the backend process exited. Installation was not started. Close other MinerDesk windows and retry, or use the GitHub setup.".into()),
        },
        None => Ok(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn an_unresponsive_listener_is_still_running() {
        // No server reads or answers HTTP. A health timeout here must not permit
        // replacing the executable while its listener remains open.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!backend_stopped(port, Some(42), |_| panic!("Listener is still open")).unwrap());
    }

    #[test]
    fn shutdown_requires_both_listener_and_known_process_to_end() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(!backend_stopped(port, Some(42), |pid| { assert_eq!(pid, 42); Some(true) }).unwrap());
        assert!(backend_stopped(port, Some(42), |_| Some(false)).unwrap());
        assert!(backend_stopped(port, Some(42), |_| None).unwrap_err().contains("could not verify"));
    }

    #[test]
    fn legacy_backend_without_pid_still_requires_a_closed_listener() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!backend_stopped(port, None, |_| unreachable!()).unwrap());
        drop(listener);
        assert!(backend_stopped(port, None, |_| unreachable!()).unwrap());
    }
}
