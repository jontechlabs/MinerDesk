use super::*;
use std::{net::UdpSocket, sync::Mutex as StdMutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
    task::JoinHandle,
};

const KEY: &str = "mobile-public-fixture-key";
struct Server {
    address: String,
    calls: Arc<StdMutex<Vec<String>>>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn new(name: &'static str, redirect_to: Option<String>) -> Self {
        // Discover the test machine's private interface without sending data.
        // Production intentionally rejects a phone's loopback as a PC address.
        let route = UdpSocket::bind("0.0.0.0:0").unwrap();
        route.connect("192.0.2.1:9").unwrap();
        let ip = route.local_addr().unwrap().ip();
        let listener = TcpListener::bind((ip, 0)).await.unwrap();
        let address = format!("http://{}/", listener.local_addr().unwrap());
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (record, accepted, proceed) = (calls.clone(), entered.clone(), release.clone());
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let (record, accepted, proceed, redirect_to) = (
                    record.clone(),
                    accepted.clone(),
                    proceed.clone(),
                    redirect_to.clone(),
                );
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    let mut buf = [0; 4096];
                    loop {
                        let count = socket.read(&mut buf).await.unwrap_or(0);
                        if count == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buf[..count]);
                        if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                            break;
                        }
                        if bytes.len() > 65536 {
                            return;
                        }
                    }
                    let request = String::from_utf8_lossy(&bytes).to_string();
                    let path = request
                        .lines()
                        .next()
                        .unwrap_or("")
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("");
                    record.lock().unwrap().push(request.clone());
                    let authorized = request
                        .to_ascii_lowercase()
                        .contains(&format!("x-minerdesk-token: {KEY}"));
                    let (status, body, extra) = if path == "/api/health" {
                        (
                            "200 OK",
                            format!(
                                r#"{{"version":"0.8.2","platform":"linux","hostname":"{name}"}}"#
                            ),
                            String::new(),
                        )
                    } else if name == "unguarded" && path == "/api/config" {
                        (
                            "200 OK",
                            "{\"miners\":[],\"schedules\":[]}".into(),
                            String::new(),
                        )
                    } else if !authorized {
                        (
                            "401 Unauthorized",
                            "{\"error\":\"Invalid key\"}".into(),
                            String::new(),
                        )
                    } else if path == "/api/config" {
                        (
                            "200 OK",
                            "{\"miners\":[],\"schedules\":[]}".into(),
                            String::new(),
                        )
                    } else if path == "/api/redirect" {
                        (
                            "302 Found",
                            "{}".into(),
                            format!("Location: {}api/status\r\n", redirect_to.unwrap()),
                        )
                    } else if path == "/api/fail" {
                        (
                            "500 Internal Server Error",
                            format!(r#"{{"error":"Do not expose {KEY}"}}"#),
                            String::new(),
                        )
                    } else {
                        if path == "/api/held-write" {
                            accepted.notify_one();
                            proceed.notified().await;
                        }
                        (
                            "200 OK",
                            format!(r#"{{"machine":"{name}"}}"#),
                            String::new(),
                        )
                    };
                    let header = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n", body.len());
                    if socket.write_all(header.as_bytes()).await.is_err() {
                        return;
                    }
                    if path == "/api/slow" {
                        tokio::time::sleep(Duration::from_millis(600)).await;
                    }
                    let _ = socket.write_all(body.as_bytes()).await;
                });
            }
        });
        Self {
            address,
            calls,
            entered,
            release,
            task,
        }
    }
}

#[test]
fn address_requires_selected_pc_and_explicit_lan_consent() {
    assert!(address("http://192.168.1.20:17888", KEY, false).is_err());
    assert!(address("http://192.168.1.20:17888", KEY, true).is_ok());
    assert!(address("http://100.90.1.2:17888", KEY, true).is_ok());
    for value in [
        "http://example.com",
        "http://127.0.0.1",
        "http://localhost",
        "http://0.0.0.0",
        "file:///tmp/a",
        "https://u:p@example.com",
        "https://example.com/admin",
        "https://example.com/#fragment",
    ] {
        assert!(address(value, KEY, true).is_err(), "{value}");
    }
    assert!(address("https://minerdesk.example.com", KEY, false).is_ok());
    assert!(address("https://example.com", "", false).is_err());
    assert!(address("https://example.com", "bad\r\nheader", false).is_err());
}

#[test]
fn pasted_token_is_removed_and_ambiguous_keys_rejected() {
    let (url, key) = address("http://192.168.1.20:17888/?token=abc", "", true).unwrap();
    assert_eq!(url.as_str(), "http://192.168.1.20:17888/");
    assert_eq!(key, "abc");
    assert!(address("https://example.com/?token=abc", "other", false).is_err());
    assert!(address("https://example.com/?token=abc&token=abc", "", false).is_err());
    assert!(address("https://example.com/?redirect=elsewhere", KEY, false).is_err());
}

#[test]
fn routes_cannot_escape_the_host_or_enroll_pc_privileges() {
    let base = Url::parse("https://minerdesk.example.com/").unwrap();
    for path in [
        "https://evil.example/api/config",
        "//evil.example/api/config",
        "/api/../other",
        "/api/%2e%2e/other",
        "/api/miners/a%2fb/start",
        "/api\\config",
        "/api/miners/a/root-authorization",
        "/api/fleet/updates/enable",
        "/api/security/defender",
        "/api/security/firewall",
        "/api/fleet/join",
    ] {
        assert!(route(&base, path, "POST").is_err(), "{path}");
    }
    for path in [
        "/api/status",
        "/api/miners/rig-1/start",
        "/api/fleet/updates/rollouts",
        "/api/fleet/rigs/abc/commands",
    ] {
        assert!(route(&base, path, "POST").is_ok(), "{path}");
    }
    assert!(route(&base, "/api/config", "CONNECT").is_err());
}

#[tokio::test]
async fn successful_session_does_not_expose_key_and_authentication_is_required() {
    let server = Server::new("fixture", None).await;
    let client = MobileClient::default();
    assert!(client
        .connect(&server.address, "wrong", true)
        .await
        .is_err());
    assert!(!client.status().await.connected);
    let status = client
        .connect(&format!("{}?token={KEY}", server.address), "", true)
        .await
        .unwrap();
    let encoded = serde_json::to_string(&status).unwrap();
    assert!(!encoded.contains(KEY));
    assert!(!encoded.contains("token="));
    assert_eq!(status.name.as_deref(), Some("fixture"));
    assert!(client.connect(&server.address, KEY, true).await.is_err());
    let value = client
        .request(
            status.connection_id.as_deref().unwrap(),
            "/api/status",
            "GET",
            None,
            1000,
        )
        .await
        .unwrap();
    assert_eq!(value["machine"], "fixture");
}

#[tokio::test]
async fn redirects_do_not_forward_keys_and_server_errors_are_redacted() {
    let other = Server::new("other", None).await;
    let server = Server::new("fixture", Some(other.address.clone())).await;
    let client = MobileClient::default();
    let id = client
        .connect(&server.address, KEY, true)
        .await
        .unwrap()
        .connection_id
        .unwrap();
    assert!(client
        .request(&id, "/api/redirect", "GET", None, 1000)
        .await
        .unwrap_err()
        .contains("redirected"));
    assert!(other.calls.lock().unwrap().is_empty());
    let error = client
        .request(&id, "/api/fail", "GET", None, 1000)
        .await
        .unwrap_err();
    assert!(!error.contains(KEY));
    assert!(error.contains("[hidden]"));
}

#[tokio::test]
async fn unguarded_reverse_proxy_cannot_pretend_to_authenticate_the_phone() {
    let server = Server::new("unguarded", None).await;
    let client = MobileClient::default();
    let error = client
        .connect(&server.address, KEY, true)
        .await
        .err()
        .unwrap();
    assert!(error.contains("did not reject an invalid access key"));
    assert!(!client.status().await.connected);
    assert_eq!(server.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn timeout_covers_the_body_and_never_retries_a_write() {
    let server = Server::new("fixture", None).await;
    let client = MobileClient::default();
    let id = client
        .connect(&server.address, KEY, true)
        .await
        .unwrap()
        .connection_id
        .unwrap();
    let error = client
        .request(&id, "/api/slow", "POST", None, 250)
        .await
        .unwrap_err();
    assert!(error.starts_with("MINERDESK_TIMEOUT"));
    assert_eq!(
        server
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("POST /api/slow "))
            .count(),
        1
    );
}

#[tokio::test]
async fn disconnect_binds_inflight_work_to_original_pc_and_rejects_stale_ids() {
    let first = Server::new("first", None).await;
    let second = Server::new("second", None).await;
    let client = Arc::new(MobileClient::default());
    let first_id = client
        .connect(&first.address, KEY, true)
        .await
        .unwrap()
        .connection_id
        .unwrap();
    let task_client = client.clone();
    let old_id = first_id.clone();
    let old_write = tokio::spawn(async move {
        task_client
            .request(&old_id, "/api/held-write", "POST", None, 5000)
            .await
    });
    first.entered.notified().await;
    client.disconnect().await;
    let new_id = client
        .connect(&second.address, KEY, true)
        .await
        .unwrap()
        .connection_id
        .unwrap();
    assert_ne!(first_id, new_id);
    first.release.notify_one();
    assert_eq!(old_write.await.unwrap().unwrap()["machine"], "first");
    assert!(client
        .request(&first_id, "/api/status", "GET", None, 1000)
        .await
        .is_err());
    assert_eq!(second.calls.lock().unwrap().len(), 3); // health, denied key, valid key
    assert_eq!(
        client
            .request(&new_id, "/api/status", "GET", None, 1000)
            .await
            .unwrap()["machine"],
        "second"
    );
}
