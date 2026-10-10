use std::{io, sync::{atomic::{AtomicBool, Ordering}, Mutex}, time::Duration};

/// Linux/macOS host the API inside the desktop process, without a Windows task.
#[derive(Default)]
pub struct DesktopWeb {
    active: AtomicBool,
    error: Mutex<String>,
}

impl DesktopWeb {
    pub fn begin(&self) -> bool {
        if self.active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() { return false; }
        if let Ok(mut error) = self.error.lock() { error.clear(); }
        true
    }
    pub fn finish(&self, error: String) {
        if let Ok(mut current) = self.error.lock() { *current = error; }
        self.active.store(false, Ordering::Release);
    }
    pub fn active(&self) -> bool { self.active.load(Ordering::Acquire) }
    pub fn error(&self) -> String { self.error.lock().map(|v| v.clone()).unwrap_or_default() }
}

pub async fn bind_after_handover(address: &str, deadline: Duration) -> io::Result<tokio::net::TcpListener> {
    let started = tokio::time::Instant::now();
    loop {
        match tokio::net::TcpListener::bind(address).await {
            Err(error) if error.kind() == io::ErrorKind::AddrInUse && started.elapsed() < deadline => {
                tokio::time::sleep(Duration::from_millis(100).min(deadline.saturating_sub(started.elapsed()))).await;
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_old_desktop_can_release_the_port_during_restart() {
        let old = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = old.local_addr().unwrap().to_string();
        tokio::spawn(async move { tokio::time::sleep(Duration::from_millis(150)).await; drop(old); });
        let new = bind_after_handover(&address, Duration::from_secs(2)).await.unwrap();
        assert_eq!(new.local_addr().unwrap().to_string(), address);
    }

    #[tokio::test]
    async fn a_permanently_occupied_port_fails_with_a_bounded_real_error() {
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let started = tokio::time::Instant::now();
        let error = bind_after_handover(&occupied.local_addr().unwrap().to_string(), Duration::from_millis(150)).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn invalid_addresses_are_not_retried_as_a_restart() {
        let started = tokio::time::Instant::now();
        assert!(bind_after_handover("127.0.0.1:99999", Duration::from_secs(8)).await.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn concurrent_start_requests_cannot_create_duplicate_servers() {
        let state = std::sync::Arc::new(DesktopWeb::default());
        let workers: Vec<_> = (0..16).map(|_| {
            let state = state.clone(); std::thread::spawn(move || state.begin())
        }).collect();
        assert_eq!(workers.into_iter().filter_map(|w| w.join().ok()).filter(|v| *v).count(), 1);
        assert!(state.active());
        state.finish("Address already in use".into());
        assert!(!state.active());
        assert_eq!(state.error(), "Address already in use");
        assert!(state.begin());
        assert!(state.error().is_empty());
    }
}
