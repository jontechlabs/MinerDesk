//! HTTPS-only machine transport, with one invitation-provided trust anchor.
use super::{protocol::*, FleetManager};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::{Certificate, Client};
use sha2::{Digest, Sha256};
use std::{net::TcpListener, sync::Arc, time::Duration};

pub(crate) fn normalize_address(address: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(address.trim())
        .map_err(|_| "Enter a complete HTTPS coordinator address".to_string())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(
            "Fleet requires an HTTPS origin without credentials, path, query or fragment".into(),
        );
    }
    Ok(url.as_str().trim_end_matches('/').into())
}
pub(crate) fn fingerprint(pem: &str) -> Result<String, String> {
    let certificates = rustls_pemfile::certs(&mut std::io::Cursor::new(pem))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Invalid Fleet TLS certificate".to_string())?;
    if certificates.len() != 1 {
        return Err("The invitation must contain exactly one TLS certificate".into());
    }
    Ok(format!("{:x}", Sha256::digest(certificates[0].as_ref())))
}
pub(crate) fn pinned_client(pem: &str) -> Result<Client, String> {
    fingerprint(pem)?;
    let certificate = Certificate::from_pem(pem.as_bytes())
        .map_err(|_| "Invalid invitation TLS certificate".to_string())?;
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .tls_built_in_root_certs(false)
        .add_root_certificate(certificate)
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(12))
        .no_proxy()
        .build()
        .map_err(|e| format!("Fleet TLS client: {e}"))
}
pub(crate) async fn response<T: serde::de::DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, String> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|n| n > 4 * 1024 * 1024)
    {
        return Err("Fleet response exceeds the size limit".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Coordinator response was interrupted".to_string())?
    {
        if bytes.len().saturating_add(chunk.len()) > 4 * 1024 * 1024 {
            return Err("Fleet response exceeds the size limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let detail = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_owned))
            .unwrap_or_else(|| status.to_string());
        return Err(format!(
            "Coordinator rejected the request: {}",
            detail.chars().take(250).collect::<String>()
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid coordinator response".into())
}
pub(crate) async fn spawn_server(
    listener: TcpListener,
    certificate: String,
    key: String,
    manager: Arc<FleetManager>,
) -> Result<axum_server::Handle, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls =
        axum_server::tls_rustls::RustlsConfig::from_pem(certificate.into_bytes(), key.into_bytes())
            .await
            .map_err(|e| format!("Fleet TLS setup: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let router = Router::new()
        .route("/fleet/v1/enroll", post(enroll))
        .route("/fleet/v1/poll", post(poll))
        .route("/fleet/v1/leave", post(leave))
        .route("/fleet/v1/view", get(view))
        .route("/fleet/v1/commands", post(command))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(manager.clone());
    let handle = axum_server::Handle::new();
    let saved = handle.clone();
    tokio::spawn(async move {
        if let Err(error) = axum_server::from_tcp_rustls(listener, tls)
            .handle(saved)
            .serve(router.into_make_service())
            .await
        {
            manager.set_error(Some(format!("Coordinator listener failed: {error}")));
        }
    });
    Ok(handle)
}
fn bearer(headers: &HeaderMap) -> Result<&str, Response> {
    // Machine credentials are never accepted from URLs, cookies or browser Origin.
    if headers.contains_key(axum::http::header::ORIGIN) {
        return Err(failure(
            StatusCode::FORBIDDEN,
            "Use the MinerDesk local Fleet interface",
        ));
    }
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .filter(|s| s.len() <= 128)
        .ok_or_else(|| {
            failure(
                StatusCode::UNAUTHORIZED,
                "Fleet machine credential required",
            )
        })
}
fn failure(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({"error":message}))).into_response()
}
async fn enroll(
    State(manager): State<Arc<FleetManager>>,
    headers: HeaderMap,
    Json(request): Json<EnrollRequest>,
) -> Response {
    if headers.contains_key(axum::http::header::ORIGIN) {
        return failure(
            StatusCode::FORBIDDEN,
            "Enrollment requires a local MinerDesk pairing action",
        );
    }
    match manager.enroll(request) {
        Ok(result) => Json(result).into_response(),
        Err(error) => failure(StatusCode::BAD_REQUEST, &error),
    }
}
async fn poll(
    State(manager): State<Arc<FleetManager>>,
    headers: HeaderMap,
    Json(request): Json<PollRequest>,
) -> Response {
    let token = match bearer(&headers) {
        Ok(t) => t,
        Err(e) => return e,
    };
    match manager.receive_poll(token, request) {
        Ok(result) => Json(result).into_response(),
        Err(error) => failure(StatusCode::UNAUTHORIZED, &error),
    }
}
async fn leave(
    State(manager): State<Arc<FleetManager>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
    body: axum::body::Bytes,
) -> Response {
    let token = match bearer(&headers) {
        Ok(token) => token,
        Err(error) => return error,
    };
    if !body.is_empty() || uri.query().is_some() {
        return failure(
            StatusCode::BAD_REQUEST,
            "Self-revocation accepts no target ID, query or body",
        );
    }
    match manager.revoke_self(token) {
        Ok(()) => Json(serde_json::json!({"ok":true})).into_response(),
        Err(error) => failure(StatusCode::UNAUTHORIZED, &error),
    }
}
async fn view(State(manager): State<Arc<FleetManager>>, headers: HeaderMap) -> Response {
    let token = match bearer(&headers) {
        Ok(t) => t,
        Err(e) => return e,
    };
    if let Err(error) = manager.authorize_controller(token) {
        return failure(StatusCode::UNAUTHORIZED, &error);
    }
    match manager.hub_view().await {
        Ok(result) => Json(result).into_response(),
        Err(error) => failure(StatusCode::BAD_REQUEST, &error),
    }
}
async fn command(
    State(manager): State<Arc<FleetManager>>,
    headers: HeaderMap,
    Json(request): Json<CommandRequest>,
) -> Response {
    let token = match bearer(&headers) {
        Ok(t) => t,
        Err(e) => return e,
    };
    if let Err(error) = manager.authorize_controller(token) {
        return failure(StatusCode::UNAUTHORIZED, &error);
    }
    match manager.queue_commands(request).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => failure(StatusCode::BAD_REQUEST, &error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_plaintext_and_credential_or_path_addresses() {
        for bad in [
            "http://127.0.0.1:17889",
            "https://user:pass@example.com",
            "https://example.com/api",
            "https://example.com/?token=secret",
            "file:///tmp/foo",
        ] {
            assert!(normalize_address(bad).is_err(), "{bad}");
        }
        assert_eq!(
            normalize_address("https://localhost:17889/").unwrap(),
            "https://localhost:17889"
        );
    }
}
