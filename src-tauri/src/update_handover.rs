//! Fail closed while the Windows updater hands the managed backend to setup.
//! An HTTP health timeout is not evidence that the executable has stopped.
#[cfg(not(target_os = "windows"))]
use std::{io::ErrorKind, net::{SocketAddr, TcpStream}, time::Duration};

#[cfg(not(target_os = "windows"))]
pub fn listener_stopped(port: u16) -> Result<bool, String> {
    match TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(300)) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == ErrorKind::ConnectionRefused => Ok(true),
        Err(error) => Err(format!("Could not verify that the Windows backend stopped: {error}. Installation was not started.")),
    }
}

#[cfg(target_os = "windows")]
pub fn listener_stopped(port: u16) -> Result<bool, String> {
    // Winsock can time out a short connect to an already closed loopback port.
    // Read the OS listener tables instead; never equate that timeout with exit.
    windows_listeners::stopped(port)
}

#[cfg(target_os = "windows")]
mod windows_listeners {
    use std::{ffi::c_void, mem::{offset_of, size_of}, ptr};
    use windows_sys::Win32::{
        Foundation::ERROR_INSUFFICIENT_BUFFER,
        NetworkManagement::IpHelper::{GetExtendedTcpTable, MIB_TCPROW_OWNER_PID,
            MIB_TCP6ROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, MIB_TCP6TABLE_OWNER_PID,
            TCP_TABLE_OWNER_PID_LISTENER},
        Networking::WinSock::{AF_INET, AF_INET6},
    };

    fn table(family: u32) -> Result<Vec<u32>, String> {
        let mut needed = 0u32;
        let status = unsafe { GetExtendedTcpTable(ptr::null_mut(), &mut needed, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0) };
        if status != ERROR_INSUFFICIENT_BUFFER {
            return Err(format!("Windows could not query TCP listeners (error {status}); installation was not started."));
        }
        for _ in 0..4 {
            if !(4..=4 * 1024 * 1024).contains(&needed) {
                return Err("Windows returned an invalid TCP listener table size; installation was not started.".into());
            }
            // DWORD alignment is sufficient for both documented OWNER_PID tables.
            // The SDK row types and offset_of! account for their actual layout.
            let mut words = vec![0u32; (needed as usize).div_ceil(size_of::<u32>())];
            let mut size = (words.len() * size_of::<u32>()) as u32;
            let status = unsafe { GetExtendedTcpTable(words.as_mut_ptr().cast::<c_void>(), &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0) };
            if status == ERROR_INSUFFICIENT_BUFFER { needed = size; continue; }
            if status != 0 {
                return Err(format!("Windows could not read TCP listeners (error {status}); installation was not started."));
            }
            if size < 4 || size as usize > words.len() * size_of::<u32>() || size % 4 != 0 {
                return Err("Windows returned an incomplete TCP listener table; installation was not started.".into());
            }
            words.truncate(size as usize / size_of::<u32>());
            return Ok(words);
        }
        Err("The Windows TCP listener table kept changing. Retry the update; installation was not started.".into())
    }

    fn any_row<T: Copy>(words: &[u32], offset: usize, matches: impl Fn(T) -> bool) -> Result<bool, String> {
        let count = *words.first().ok_or("Missing Windows TCP listener count")? as usize;
        let end = count.checked_mul(size_of::<T>()).and_then(|size| offset.checked_add(size))
            .ok_or("Invalid Windows TCP listener count")?;
        if end > std::mem::size_of_val(words) {
            return Err("Windows returned truncated TCP listener rows; installation was not started.".into());
        }
        for index in 0..count {
            // All offsets and row lengths were checked against initialized bytes.
            let row = unsafe { words.as_ptr().cast::<u8>().add(offset + index * size_of::<T>()).cast::<T>().read_unaligned() };
            if matches(row) { return Ok(true); }
        }
        Ok(false)
    }

    pub(super) fn stopped(port: u16) -> Result<bool, String> {
        let ipv4 = table(u32::from(AF_INET))?;
        if any_row::<MIB_TCPROW_OWNER_PID>(&ipv4, offset_of!(MIB_TCPTABLE_OWNER_PID, table), |row| {
            row.dwState == 2 && u16::from_be(row.dwLocalPort as u16) == port
                && (row.dwLocalAddr == 0 || row.dwLocalAddr.to_ne_bytes() == [127, 0, 0, 1])
        })? { return Ok(false); }
        let ipv6 = table(u32::from(AF_INET6))?;
        let mapped_loopback = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 127, 0, 0, 1];
        let mut ipv6_loopback = [0; 16];
        ipv6_loopback[15] = 1;
        let listening = any_row::<MIB_TCP6ROW_OWNER_PID>(&ipv6, offset_of!(MIB_TCP6TABLE_OWNER_PID, table), |row| {
            row.dwState == 2 && u16::from_be(row.dwLocalPort as u16) == port
                && (row.ucLocalAddr == [0; 16] || row.ucLocalAddr == mapped_loopback || row.ucLocalAddr == ipv6_loopback)
        })?;
        Ok(!listening)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn malformed_tables_fail_closed_without_reading_past_rows() {
            assert!(any_row::<MIB_TCPROW_OWNER_PID>(&[], 4, |_| true).is_err());
            assert!(any_row::<MIB_TCPROW_OWNER_PID>(&[1], 4, |_| true).is_err());
            assert!(any_row::<MIB_TCP6ROW_OWNER_PID>(&[u32::MAX], 4, |_| true).is_err());
            assert!(!any_row::<MIB_TCPROW_OWNER_PID>(&[0], 4, |_| true).unwrap());
        }
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

    #[cfg(target_os = "windows")]
    #[test]
    fn native_ipv6_loopback_listener_is_not_mistaken_for_a_stopped_backend() {
        let listener = TcpListener::bind(("::1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!listener_stopped(port).unwrap());
        drop(listener);
        assert!(listener_stopped(port).unwrap());
    }
}
