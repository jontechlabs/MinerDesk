//! Remote-only transport. No miner processes, local service, elevated actions,
//! filesystem access or persisted credentials are exposed to the webview.
use futures_util::StreamExt;
use reqwest::{header, redirect, Client, Method, Response};
use serde::Serialize;
use serde_json::Value;
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use url::{Host, Url};

#[cfg(test)]
mod tests;

const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_BODY: usize = 1024 * 1024;
const TIMEOUT: &str = "MINERDESK_TIMEOUT: The request timed out. An action may already have been applied; refresh before retrying.";

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub connected: bool,
    pub connecting: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

// Deliberately not Debug/Serialize: the credential never leaves native memory.
struct Session {
    id: String,
    base: Url,
    key: String,
    client: Client,
    name: String,
    version: String,
}
impl Session {
    fn status(&self) -> ConnectionStatus {
        ConnectionStatus {
            connected: true,
            connecting: false,
            connection_id: Some(self.id.clone()),
            address: Some(self.base.as_str().into()),
            name: Some(self.name.clone()),
            version: Some(self.version.clone()),
        }
    }
}
#[derive(Default)]
struct State {
    session: Option<Arc<Session>>,
    generation: u64,
    connecting: bool,
}
#[derive(Default)]
pub struct MobileClient {
    state: Mutex<State>,
}

fn local_http_host(host: Host<&str>) -> bool {
    match host {
        Host::Ipv4(ip) => {
            ip.is_private() || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
        }
        Host::Ipv6(ip) => ip.segments()[0] & 0xfe00 == 0xfc00,
        Host::Domain(name) => !name.contains('.') || name.ends_with(".local"),
    }
}

fn address(input: &str, key: &str, allow_insecure_lan: bool) -> Result<(Url, String), String> {
    if input.len() > 4096 || input.chars().any(char::is_control) {
        return Err("Enter a valid MinerDesk address.".into());
    }
    let mut base = Url::parse(input.trim())
        .map_err(|_| "Use a complete http:// or https:// MinerDesk address.")?;
    if !matches!(base.scheme(), "http" | "https")
        || !base.username().is_empty()
        || base.password().is_some()
        || base.fragment().is_some()
        || !matches!(base.path(), "" | "/")
    {
        return Err(
            "Use the MinerDesk server address without a path, password or fragment.".into(),
        );
    }
    let host = base.host().ok_or("The server address has no host.")?;
    if base
        .host_str()
        .is_some_and(|name| name.eq_ignore_ascii_case("localhost") || name.ends_with(".localhost"))
        || base
            .host_str()
            .and_then(|name| name.trim_matches(['[', ']']).parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback() || ip.is_unspecified())
    {
        return Err(
            "Use your PC's network address, not localhost or 0.0.0.0 on this phone.".into(),
        );
    }
    if base.scheme() == "http" && (!allow_insecure_lan || !local_http_host(host)) {
        return Err("Use HTTPS for public addresses. HTTP is available only for a trusted local network or VPN after explicit confirmation.".into());
    }
    let query: Vec<_> = base
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if query.len() > 1 || query.iter().any(|(name, _)| name != "token") {
        return Err(
            "Only the optional MinerDesk token may appear in a pasted connection link.".into(),
        );
    }
    let pasted_key = query.first().map(|(_, key)| key.as_str()).unwrap_or("");
    let explicit_key = key.trim();
    if !pasted_key.is_empty() && !explicit_key.is_empty() && pasted_key != explicit_key {
        return Err("The pasted link and access-key field contain different keys.".into());
    }
    let key = if explicit_key.is_empty() {
        pasted_key
    } else {
        explicit_key
    };
    if key.is_empty() || key.len() > 4096 || key.chars().any(char::is_control) {
        return Err("Enter the PC's LAN access key from MinerDesk Settings. A Fleet invitation is a different key.".into());
    }
    let key = key.to_owned();
    base.set_query(None);
    base.set_path("/");
    Ok((base, key))
}

fn route(base: &Url, path: &str, method: &str) -> Result<(Url, Method), String> {
    if path.len() > 4096
        || !path.starts_with("/api/")
        || path.contains('\\')
        || path.contains('#')
        || path.chars().any(char::is_control)
    {
        return Err("Only MinerDesk API requests are supported.".into());
    }
    let url = base.join(path).map_err(|_| "Invalid API path.")?;
    if url.origin() != base.origin() || !url.path().starts_with("/api/") {
        return Err("The request cannot leave the selected MinerDesk server.".into());
    }
    let lower = url.path().to_ascii_lowercase();
    if lower.contains('%') || lower.contains("//") {
        // Profile/rig IDs are limited to simple identifiers by the server. Do
        // not admit ambiguous encoded separators into privilege-route checks.
        return Err("The API path contains an unsupported encoded segment.".into());
    }
    let method = match method {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "DELETE" => Method::DELETE,
        _ => return Err("Unsupported API method.".into()),
    };
    if lower.contains("/root-authorization")
        || lower.starts_with("/api/security/firewall")
        || lower.starts_with("/api/security/defender")
        || lower.starts_with("/api/host/")
        || (lower.starts_with("/api/fleet/updates/")
            && !lower.starts_with("/api/fleet/updates/rollouts"))
        || (method != Method::GET
            && matches!(
                lower.as_str(),
                "/api/fleet/create"
                    | "/api/fleet/join"
                    | "/api/fleet/invitations"
                    | "/api/fleet/disconnect"
            ))
    {
        return Err("This authorization must be managed locally on the PC.".into());
    }
    Ok((url, method))
}

fn transport_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        TIMEOUT.into()
    } else if error.is_connect() {
        "Could not reach MinerDesk. Check the address, LAN access, VPN and HTTPS certificate."
            .into()
    } else {
        "The MinerDesk connection failed. Refresh before retrying any action.".into()
    }
}

async fn json_response(response: Response, key: &str) -> Result<Value, String> {
    let status = response.status();
    if status.is_redirection() {
        return Err("The server redirected this request. Enter its final HTTPS address; the access key was not forwarded.".into());
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_RESPONSE as u64)
    {
        return Err("The response is too large for the mobile client.".into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(transport_error)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
            return Err("The response is too large for the mobile client.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err("MinerDesk refused access. Check the LAN access key or the PC's authorization settings.".into());
    }
    let value: Value = if bytes.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_slice(&bytes).map_err(|_| "The address did not return MinerDesk JSON. Use the web interface port, usually 17888, rather than the Fleet pairing port.")?
    };
    if !status.is_success() {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("MinerDesk rejected the request.");
        return Err(error
            .replace(key, "[hidden]")
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(1500)
            .collect());
    }
    Ok(value)
}

impl MobileClient {
    pub async fn status(&self) -> ConnectionStatus {
        let state = self.state.lock().await;
        state
            .session
            .as_ref()
            .map(|session| session.status())
            .unwrap_or(ConnectionStatus {
                connecting: state.connecting,
                ..Default::default()
            })
    }

    pub async fn disconnect(&self) {
        let mut state = self.state.lock().await;
        state.generation = state.generation.wrapping_add(1);
        state.connecting = false;
        state.session = None;
    }

    pub async fn connect(
        &self,
        input: &str,
        key: &str,
        allow_insecure_lan: bool,
    ) -> Result<ConnectionStatus, String> {
        let (base, key) = address(input, key, allow_insecure_lan)?;
        let generation = {
            let mut state = self.state.lock().await;
            if state.session.is_some() {
                return Err("Disconnect the current PC before selecting another one.".into());
            }
            if state.connecting {
                return Err("A connection attempt is already in progress.".into());
            }
            state.connecting = true;
            state.generation
        };
        let result = self.probe(base, key).await;
        let mut state = self.state.lock().await;
        if state.generation != generation {
            return Err("The connection attempt was cancelled.".into());
        }
        state.connecting = false;
        let session = Arc::new(result?);
        let status = session.status();
        state.session = Some(session);
        Ok(status)
    }

    async fn probe(&self, base: Url, key: String) -> Result<Session, String> {
        let mut headers = header::HeaderMap::new();
        let mut credential = header::HeaderValue::from_str(&key)
            .map_err(|_| "The access key contains invalid characters.")?;
        credential.set_sensitive(true);
        headers.insert("x-minerdesk-token", credential);
        let client = Client::builder()
            .default_headers(headers)
            .redirect(redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .user_agent("MinerDeskMobile/0.1.0")
            .build()
            .map_err(|_| "Could not initialize the mobile connection.")?;
        let health = json_response(
            client
                .get(base.join("api/health").map_err(|_| "Invalid address.")?)
                .send()
                .await
                .map_err(transport_error)?,
            &key,
        )
        .await?;
        let version = health
            .get("version")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && v.len() <= 64)
            .ok_or("This address is not a compatible MinerDesk server.")?
            .to_owned();
        if !health.get("platform").is_some_and(Value::is_string) {
            return Err("This address is not a compatible MinerDesk server.".into());
        }
        // A loopback reverse proxy can inherit the desktop API's local trust
        // shortcut. Refuse such an unguarded endpoint rather than falsely
        // claiming the phone authenticated: a deliberately wrong key must fail.
        let rejected = client
            .get(base.join("api/config").map_err(|_| "Invalid address.")?)
            .header(
                "x-minerdesk-token",
                format!("mobile-access-check-{}", uuid::Uuid::new_v4()),
            )
            .send()
            .await
            .map_err(transport_error)?;
        if !matches!(rejected.status().as_u16(), 401 | 403) {
            return Err("This endpoint did not reject an invalid access key. Connect directly over your LAN/VPN, or configure the HTTPS gateway to enforce authentication.".into());
        }
        drop(rejected);
        // Health can be public. Config must independently accept the real key.
        let config = json_response(
            client
                .get(base.join("api/config").map_err(|_| "Invalid address.")?)
                .send()
                .await
                .map_err(transport_error)?,
            &key,
        )
        .await?;
        if !config.get("miners").is_some_and(Value::is_array)
            || !config.get("schedules").is_some_and(Value::is_array)
        {
            return Err("This address is not a compatible MinerDesk server.".into());
        }
        let name = health
            .get("hostname")
            .or_else(|| health.get("name"))
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && v.len() <= 256)
            .unwrap_or(base.host_str().unwrap_or("MinerDesk"))
            .to_owned();
        Ok(Session {
            id: uuid::Uuid::new_v4().to_string(),
            base,
            key,
            client,
            name,
            version,
        })
    }

    pub async fn request(
        &self,
        connection_id: &str,
        path: &str,
        method: &str,
        body: Option<String>,
        timeout_ms: u64,
    ) -> Result<Value, String> {
        let session = {
            let state = self.state.lock().await;
            state
                .session
                .as_ref()
                .filter(|session| session.id == connection_id)
                .cloned()
                .ok_or("This connection is no longer active. Reconnect to the intended PC.")?
        };
        let (url, method) = route(&session.base, path, method)?;
        if body.as_ref().is_some_and(|body| body.len() > MAX_BODY) {
            return Err("The request is too large for the mobile client.".into());
        }
        if !(250..=300_000).contains(&timeout_ms) {
            return Err("Invalid request timeout.".into());
        }
        let mut request = session
            .client
            .request(method, url)
            .timeout(Duration::from_millis(timeout_ms));
        if let Some(body) = body {
            serde_json::from_str::<Value>(&body).map_err(|_| "The request body must be JSON.")?;
            request = request
                .header(header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        // No retries. A timed-out write may have reached this original session;
        // it can never be redirected to a newly selected PC.
        json_response(request.send().await.map_err(transport_error)?, &session.key).await
    }
}
