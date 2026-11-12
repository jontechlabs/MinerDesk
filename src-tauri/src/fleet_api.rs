//! Local UI facade for Fleet. Network agents use the separate authenticated TLS
//! service; no request is forwarded into the trusted localhost mining API.
use super::*;
use axum::{body::Bytes, extract::DefaultBodyLimit};
use fleet::protocol::{CommandRequest, CreateHub, FleetView, InviteRequest, JoinRequest};
use serde::de::DeserializeOwned;

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route("/api/fleet", get(view))
        .route("/api/fleet/:action", post(action))
        .route("/api/fleet/rigs/:id", get(rig))
        .route("/api/fleet/rigs/:id/config", get(rig_config))
        .route("/api/fleet/rigs/:id/revoke", post(revoke))
        .route("/api/fleet/commands/:id", get(receipt))
        .route("/api/fleet/updates", get(updates_status))
        .route("/api/fleet/updates/:action", post(updates_action))
        .merge(super::fleet_rollouts_api::routes())
        .layer(DefaultBodyLimit::max(256 * 1024))
}

fn denied(message: &str) -> Response {
    (StatusCode::FORBIDDEN, Json(json!({"error":message}))).into_response()
}

fn explicit_token(headers: &HeaderMap, expected: Option<&str>) -> bool {
    let Some(expected) = expected.filter(|key| !key.is_empty()) else {
        return false;
    };
    headers
        .get("x-minerdesk-token")
        .and_then(|v| v.to_str().ok())
        == Some(expected)
        || headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            == Some(expected)
}

// A browser on another origin must not turn the same-machine trust shortcut
// into an enrollment, network listener, or cross-rig control capability.
fn local_setup(headers: &HeaderMap, port: u16) -> bool {
    headers
        .get("x-minerdesk-local")
        .and_then(|v| v.to_str().ok())
        == Some("1")
        && root_authorization_origin(headers, port)
}

fn same_remote_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let (Ok(origin), Some(host)) = (
        origin.to_str(),
        headers.get(header::HOST).and_then(|v| v.to_str().ok()),
    ) else {
        return false;
    };
    let Ok(origin) = reqwest::Url::parse(origin) else {
        return false;
    };
    if !matches!(origin.scheme(), "http" | "https")
        || !origin.username().is_empty()
        || origin.password().is_some()
    {
        return false;
    }
    let Ok(expected) = reqwest::Url::parse(&format!("{}://{host}", origin.scheme())) else {
        return false;
    };
    origin.origin() == expected.origin()
}

pub(super) fn access(headers: &HeaderMap, state: &WebState, setup: bool) -> Result<bool, Response> {
    api_auth(headers, state)?;
    let local = local_setup(headers, state.core.config().web.desktop_api_port);
    if setup && !local {
        return Err(denied("Fleet association, invitations and trust changes must be approved in MinerDesk on this computer"));
    }
    if !local && !(explicit_token(headers, state.token.as_deref()) && same_remote_origin(headers)) {
        return Err(denied("Open Fleet in the local MinerDesk application, or authenticate to this instance's web interface"));
    }
    Ok(local)
}

pub(super) fn manager(state: &WebState) -> Result<&Arc<fleet::FleetManager>, Response> {
    state.fleet.as_ref().ok_or_else(|| {
        (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"Fleet could not load its settings. Local mining is available; see the backend log for Fleet diagnostics."}))).into_response()
    })
}

fn parse<T: DeserializeOwned>(body: &Bytes) -> Result<T, Response> {
    serde_json::from_slice(body).map_err(|_| {
        (StatusCode::BAD_REQUEST,Json(json!({"error":"Invalid Fleet request. Check the required fields and refresh the interface."}))).into_response()
    })
}

pub(super) fn response<T: Serialize>(result: Result<T, String>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))).into_response(),
    }
}

fn view_response(result: Result<FleetView, String>, local: bool) -> Response {
    response(result.map(|mut value| {
        value.local_setup_allowed = local;
        value.controller_allowed = true;
        value
    }))
}

async fn view(AxumState(state): AxumState<WebState>, headers: HeaderMap) -> Response {
    let local = match access(&headers, &state, false) {
        Ok(local) => local,
        Err(error) => return error,
    };
    let service = match manager(&state) {
        Ok(service) => service,
        Err(error) => return error,
    };
    view_response(service.view().await, local)
}

async fn updates_status(AxumState(state): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(error) = access(&headers, &state, true) { return error; }
    Json(state.core.fleet_updates.status()).into_response()
}

async fn updates_action(
    AxumState(state): AxumState<WebState>, headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>, AxumPath(action): AxumPath<String>,
) -> Response {
    if let Err(error) = access(&headers, &state, true) { return error; }
    // Also inspect socket metadata directly at the privilege-changing endpoint.
    // A remote configured API token never grants local update opt-in or root.
    if !peer.ip().is_loopback() || !root_authorization_origin(&headers, state.core.config().web.desktop_api_port) {
        return denied("Change remote update permissions in MinerDesk on this computer");
    }
    let service = Arc::clone(&state.core.fleet_updates);
    match action.as_str() {
        "enable" => response(service.set_enabled(true)),
        "disable" => response(service.set_enabled(false)),
        "enroll" | "revoke" => {
            let revoke = action == "revoke";
            match tokio::task::spawn_blocking(move || service.enrollment(revoke)).await {
                Ok(result) => response(result),
                Err(_) => response::<fleet_updates::UpdateStatus>(Err("Local update authorization could not finish".into())),
            }
        }
        _ => (StatusCode::NOT_FOUND, Json(json!({"error":"Unknown local update operation"}))).into_response(),
    }
}

async fn action(
    AxumState(state): AxumState<WebState>,
    headers: HeaderMap,
    AxumPath(action): AxumPath<String>,
    body: Bytes,
) -> Response {
    let setup = matches!(
        action.as_str(),
        "create" | "disable" | "invite" | "join" | "leave"
    );
    let local = match access(&headers, &state, setup) {
        Ok(local) => local,
        Err(error) => return error,
    };
    let service = match manager(&state) {
        Ok(service) => service,
        Err(error) => return error,
    };
    let result = match action.as_str() {
        "create" => {
            let request: CreateHub = match parse(&body) {
                Ok(value) => value,
                Err(error) => return error,
            };
            view_response(service.create(request).await, local)
        }
        "disable" => view_response(service.disable().await, local),
        "join" => {
            let request: JoinRequest = match parse(&body) {
                Ok(value) => value,
                Err(error) => return error,
            };
            view_response(service.join(request).await, local)
        }
        "leave" => view_response(service.leave().await, local),
        "invite" => {
            let request: InviteRequest = match parse(&body) {
                Ok(value) => value,
                Err(error) => return error,
            };
            response(service.invite(request).await)
        }
        "discover" => response(service.discover().await),
        "commands" => {
            let request: CommandRequest = match parse(&body) {
                Ok(value) => value,
                Err(error) => return error,
            };
            response(service.command(request).await)
        }
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"Unknown Fleet operation"})),
        )
            .into_response(),
    };
    // Activation opts this running backend into autonomous operation until Quit.
    // Leaving/disabling a fleet removes network trust, not a local mining session
    // just because its last desktop window was already closed.
    state
        .core
        .fleet_enabled
        .fetch_or(service.enabled(), Ordering::SeqCst);
    result
}

async fn rig(
    AxumState(state): AxumState<WebState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if let Err(error) = access(&headers, &state, false) {
        return error;
    }
    match manager(&state) {
        Ok(service) => response(service.rig(&id).await),
        Err(error) => error,
    }
}

async fn rig_config(
    AxumState(state): AxumState<WebState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if let Err(error) = access(&headers, &state, false) {
        return error;
    }
    match manager(&state) {
        Ok(service) => response(service.rig_config(&id).await),
        Err(error) => error,
    }
}

async fn receipt(
    AxumState(state): AxumState<WebState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if let Err(error) = access(&headers, &state, false) {
        return error;
    }
    match manager(&state) {
        Ok(service) => response(service.receipt(&id).await),
        Err(error) => error,
    }
}

async fn revoke(
    AxumState(state): AxumState<WebState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let local = match access(&headers, &state, true) {
        Ok(local) => local,
        Err(error) => return error,
    };
    match manager(&state) {
        Ok(service) => view_response(service.revoke(&id).await, local),
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, Extension};
    use tower::ServiceExt;

    fn headers(origin: Option<&str>, host: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, host.parse().unwrap());
        if let Some(origin) = origin {
            headers.insert(header::ORIGIN, origin.parse().unwrap());
        }
        headers
    }

    #[test]
    fn cross_origin_browser_cannot_use_same_machine_enrollment_trust() {
        let mut values = headers(Some("https://attacker.example"), "127.0.0.1:17888");
        values.insert("x-minerdesk-local", "1".parse().unwrap());
        assert!(!local_setup(&values, 17888));
        values.insert(header::ORIGIN, "http://127.0.0.1:17888".parse().unwrap());
        assert!(local_setup(&values, 17888));
        values.remove("x-minerdesk-local");
        assert!(!local_setup(&values, 17888));
    }

    #[test]
    fn remote_controller_requires_an_explicit_configured_token() {
        let mut values = headers(None, "192.0.2.1:17888");
        assert!(!explicit_token(&values, None));
        assert!(!explicit_token(&values, Some("expected")));
        values.insert("x-minerdesk-token", "wrong".parse().unwrap());
        assert!(!explicit_token(&values, Some("expected")));
        values.insert("x-minerdesk-token", "expected".parse().unwrap());
        assert!(explicit_token(&values, Some("expected")));
        assert!(!explicit_token(&values, Some("")));
    }

    #[test]
    fn remote_web_requests_are_limited_to_the_instance_origin() {
        assert!(same_remote_origin(&headers(
            Some("http://192.0.2.1:17888"),
            "192.0.2.1:17888"
        )));
        assert!(!same_remote_origin(&headers(
            Some("https://attacker.example"),
            "192.0.2.1:17888"
        )));
        assert!(!same_remote_origin(&headers(
            Some("null"),
            "192.0.2.1:17888"
        )));
    }

    #[test]
    fn all_local_mining_endpoints_reject_cross_site_browser_origins() {
        let directory = tempfile::tempdir().unwrap();
        let core = CoreState::load_from_path(directory.path().join("config.json")).unwrap();
        let state = WebState {
            core,
            fleet: None,
            token: None,
            headless: true,
            desktop_owned: false,
            desktop_leases: Arc::new(Mutex::new(DesktopLeaseState::default())),
        };
        let mut values = headers(Some("https://attacker.example"), "127.0.0.1:17888");
        values.insert("x-minerdesk-local", "1".parse().unwrap());
        assert_eq!(
            api_auth(&values, &state).unwrap_err().status(),
            StatusCode::FORBIDDEN
        );
        values.insert(header::ORIGIN, "http://127.0.0.1:17888".parse().unwrap());
        assert!(api_auth(&values, &state).is_ok());
        values.insert(header::ORIGIN, "http://tauri.localhost".parse().unwrap());
        assert!(api_auth(&values, &state).is_ok());
        values.remove(header::ORIGIN);
        assert!(api_auth(&values, &state).is_ok());
        values.remove("x-minerdesk-local");
        assert_eq!(
            api_auth(&values, &state).unwrap_err().status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn a_token_or_forged_header_cannot_enable_remote_update_policy() {
        let directory = tempfile::tempdir().unwrap();
        let core = CoreState::load_from_path(directory.path().join("config.json")).unwrap();
        let state = WebState { core: Arc::clone(&core), fleet: None, token: Some("fixture-token".into()), headless: true,
            desktop_owned: false, desktop_leases: Arc::new(Mutex::new(DesktopLeaseState::default())) };
        for (peer, origin, expected) in [
            ("192.0.2.15:45000", "http://127.0.0.1:17888", StatusCode::FORBIDDEN),
            ("127.0.0.1:45000", "https://attacker.example", StatusCode::FORBIDDEN),
            ("127.0.0.1:45000", "http://127.0.0.1:17888", StatusCode::OK),
        ] {
            let app = routes().with_state(state.clone()).layer(middleware::from_fn(mark_local_request))
                .layer(Extension(ConnectInfo(peer.parse::<SocketAddr>().unwrap())));
            let result = app.oneshot(Request::builder().method("POST").uri("/api/fleet/updates/enable")
                .header("x-minerdesk-token", "fixture-token").header("x-minerdesk-local", "1")
                .header(header::HOST, "127.0.0.1:17888").header(header::ORIGIN, origin).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(result.status(), expected, "{peer} {origin}");
            assert_eq!(core.fleet_updates.status().enabled, expected == StatusCode::OK);
        }
    }
}
