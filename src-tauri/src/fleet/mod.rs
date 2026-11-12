//! Integrated coordinator and outbound agent. Local lifecycle remains authoritative.
pub mod core;
mod discovery;
mod persistence;
pub mod protocol;
mod rollouts;
#[cfg(test)]
mod tests;
mod transport;
mod validation;
use base64::Engine;
use persistence::*;
pub use protocol::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    net::{SocketAddr, TcpListener},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

pub trait FleetCore: Send + Sync {
    fn snapshot(&self) -> Result<Value, String>;
    fn config(&self) -> Result<Value, String>;
    fn execute(&self, operation: &FleetOperation, expires_at: u64) -> Result<Value, String>;
}
struct Runtime {
    server: Option<axum_server::Handle>,
    stop: Option<watch::Sender<bool>>,
}
pub struct FleetManager {
    store: Mutex<Store>,
    core: Arc<dyn FleetCore>,
    runtime: Mutex<Runtime>,
    transition: tokio::sync::Mutex<()>,
    started: AtomicBool,
    error: Mutex<Option<String>>,
    cached: Mutex<Option<FleetView>>,
    suggested: String,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn secret(prefix: &str) -> String {
    format!(
        "{prefix}_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |diff, (x, y)| diff | (x ^ y))
            == 0
}
fn valid_name(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 80 && !value.chars().any(char::is_control)
}
fn lock_error() -> String {
    "Fleet state is unavailable".into()
}

impl FleetManager {
    pub fn new(config_path: PathBuf, core: Arc<dyn FleetCore>) -> Result<Arc<Self>, String> {
        let mut store = Store::load(&config_path)?;
        // A process crash may happen after a side effect but before its receipt.
        // Never rerun such a command merely because its acknowledgement was lost.
        store.transaction(|data| {
            for execution in data.executed.values_mut() {
                if execution.receipt.state == "running" {
                    execution.receipt.state = "failed".into();
                    execution.receipt.error = Some(
                        "Previous command outcome is unknown after restart; it was not retried"
                            .into(),
                    );
                }
            }
            Ok(())
        })?;
        Ok(Arc::new(Self {
            store: Mutex::new(store),
            core,
            runtime: Mutex::new(Runtime {
                server: None,
                stop: None,
            }),
            transition: tokio::sync::Mutex::new(()),
            started: AtomicBool::new(false),
            error: Mutex::new(None),
            cached: Mutex::new(None),
            suggested: suggested_url(),
        }))
    }
    fn data(&self) -> Result<Persistent, String> {
        Ok(self.store.lock().map_err(|_| lock_error())?.data.clone())
    }
    fn change<T>(
        &self,
        change: impl FnOnce(&mut Persistent) -> Result<T, String>,
    ) -> Result<T, String> {
        self.store
            .lock()
            .map_err(|_| lock_error())?
            .transaction(change)
    }
    pub fn enabled(&self) -> bool {
        self.store
            .lock()
            .map(|s| s.data.member.is_some() || s.data.hub.as_ref().is_some_and(|h| h.enabled))
            .unwrap_or(false)
    }
    pub fn set_error(&self, error: Option<String>) {
        if let Ok(mut current) = self.error.lock() {
            *current = error;
        }
    }
    pub fn local_status(&self) -> FleetSetup {
        match self.store.lock() {
            Ok(store) => {
                let d = &store.data;
                let hub = d.hub.as_ref().filter(|h| h.enabled);
                let member = d.member.as_ref();
                FleetSetup {
                    rig_id: d.rig_id.clone(),
                    name: d.name.clone(),
                    mode: if hub.is_some() {
                        "hub"
                    } else if member.is_some() {
                        "member"
                    } else {
                        "standalone"
                    }
                    .into(),
                    hub_url: hub
                        .map(|h| h.address.clone())
                        .or_else(|| member.map(|m| m.address.clone())),
                    fleet_id: hub
                        .map(|h| h.fleet_id.clone())
                        .or_else(|| member.map(|m| m.fleet_id.clone())),
                    hub_enabled: hub.is_some(),
                    tls_fingerprint: hub
                        .map(|h| h.fingerprint.clone())
                        .or_else(|| member.map(|m| m.fingerprint.clone())),
                    error: self.error.lock().ok().and_then(|e| e.clone()),
                    suggested_url: self.suggested.clone(),
                }
            }
            Err(_) => FleetSetup {
                rig_id: String::new(),
                name: String::new(),
                mode: "standalone".into(),
                hub_url: None,
                fleet_id: None,
                hub_enabled: false,
                tls_fingerprint: None,
                error: Some(lock_error()),
                suggested_url: self.suggested.clone(),
            },
        }
    }
    pub async fn start(self: &Arc<Self>) -> Result<(), String> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let _transition = self.transition.lock().await;
        let result = self.resume_runtime().await;
        if let Err(ref error) = result {
            self.set_error(Some(error.clone()));
        }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3));
            loop {
                interval.tick().await;
                let Some(manager) = weak.upgrade() else { break };
                if let Err(error) = manager.tick_rollouts().await {
                    manager.set_error(Some(error));
                }
            }
        });
        result
    }
    fn stop_runtime(&self) {
        if let Ok(mut runtime) = self.runtime.lock() {
            if let Some(stop) = runtime.stop.take() {
                let _ = stop.send(true);
            }
            if let Some(server) = runtime.server.take() {
                server.graceful_shutdown(Some(Duration::from_secs(2)));
            }
        }
    }
    async fn resume_runtime(self: &Arc<Self>) -> Result<(), String> {
        self.stop_runtime();
        let data = self.data()?;
        let (stop, receiver) = watch::channel(false);
        if let Some(hub) = data.hub.filter(|h| h.enabled) {
            let listener = TcpListener::bind(&hub.listen)
                .map_err(|e| format!("Cannot open coordinator HTTPS listener: {e}"))?;
            let handle = transport::spawn_server(
                listener,
                hub.certificate_pem.clone(),
                hub.private_key_pem.clone(),
                self.clone(),
            )
            .await?;
            let ip: SocketAddr = hub
                .listen
                .parse()
                .map_err(|_| "Invalid stored coordinator listener")?;
            if !ip.ip().is_loopback() {
                let hint = DiscoveredHub {
                    fleet_id: hub.fleet_id.clone(),
                    name: hub.name.clone(),
                    address: self.lan_address(&hub),
                    certificate_fingerprint: hub.fingerprint.clone(),
                };
                let receiver = receiver.clone();
                tokio::spawn(async move {
                    let _ = discovery::responder(hint, ip.ip(), receiver).await;
                });
            }
            let mut runtime = self.runtime.lock().map_err(|_| lock_error())?;
            runtime.server = Some(handle);
            runtime.stop = Some(stop);
        } else if data.member.is_some() {
            self.runtime.lock().map_err(|_| lock_error())?.stop = Some(stop);
            let manager = self.clone();
            tokio::spawn(async move {
                manager.agent_loop(receiver).await;
            });
        }
        Ok(())
    }
    pub async fn create(self: &Arc<Self>, request: CreateHub) -> Result<FleetView, String> {
        let _transition = self.transition.lock().await;
        let existing = self.data()?;
        if existing.member.is_some() || existing.hub.is_some() {
            return Err("Leave or disable the existing Fleet before creating a coordinator".into());
        }
        if !valid_name(&request.name) {
            return Err("Choose a coordinator name of 1–80 characters".into());
        }
        let mut address = transport::normalize_address(&request.advertise_url)?;
        let bind: SocketAddr = request
            .listen
            .parse()
            .map_err(|_| "Enter an IP address and port for the coordinator listener")?;
        let listener =
            TcpListener::bind(bind).map_err(|e| format!("Coordinator port unavailable: {e}"))?;
        let actual = listener.local_addr().map_err(|e| e.to_string())?;
        let mut url = reqwest::Url::parse(&address).map_err(|e| e.to_string())?;
        if bind.port() == 0 {
            url.set_port(Some(actual.port()))
                .map_err(|_| "Invalid HTTPS port")?;
            address = url.as_str().trim_end_matches('/').into();
        }
        let hostname = url
            .host_str()
            .ok_or("Coordinator host is missing")?
            .trim_matches(['[', ']'])
            .to_string();
        let mut names = vec![hostname, "localhost".into(), "127.0.0.1".into()];
        if let Ok(suggested) = reqwest::Url::parse(&self.suggested) {
            if let Some(host) = suggested.host_str() {
                names.push(host.trim_matches(['[', ']']).into());
            }
        }
        if !actual.ip().is_unspecified() {
            names.push(actual.ip().to_string());
        }
        names.sort();
        names.dedup();
        let certificate = rcgen::generate_simple_self_signed(names)
            .map_err(|e| format!("Creating coordinator TLS identity: {e}"))?;
        let pem = certificate.cert.pem();
        let fingerprint = transport::fingerprint(&pem)?;
        let hub = HubRecord {
            enabled: true,
            fleet_id: uuid::Uuid::new_v4().to_string(),
            name: request.name.trim().into(),
            address,
            listen: actual.to_string(),
            certificate_pem: pem,
            private_key_pem: certificate.key_pair.serialize_pem(),
            fingerprint,
            invitations: vec![],
            rigs: BTreeMap::new(),
            commands: BTreeMap::new(),
            rollouts: BTreeMap::new(),
        };
        // Port reservation is retained until the HTTPS server owns it.
        self.change(|d| {
            d.name = request.name.trim().into();
            d.hub = Some(hub.clone());
            Ok(())
        })?;
        match transport::spawn_server(
            listener,
            hub.certificate_pem.clone(),
            hub.private_key_pem.clone(),
            self.clone(),
        )
        .await
        {
            Ok(handle) => {
                let (stop, receiver) = watch::channel(false);
                {
                    let mut runtime = self.runtime.lock().map_err(|_| lock_error())?;
                    runtime.server = Some(handle);
                    runtime.stop = Some(stop);
                }
                if !actual.ip().is_loopback() {
                    let hint = DiscoveredHub {
                        fleet_id: hub.fleet_id.clone(),
                        name: hub.name.clone(),
                        address: self.lan_address(&hub),
                        certificate_fingerprint: hub.fingerprint.clone(),
                    };
                    tokio::spawn(async move {
                        let _ = discovery::responder(hint, actual.ip(), receiver).await;
                    });
                }
            }
            Err(error) => {
                let _ = self.change(|d| {
                    d.hub = None;
                    Ok(())
                });
                return Err(error);
            }
        }
        self.set_error(None);
        self.hub_view().await
    }
    pub async fn disable(self: &Arc<Self>) -> Result<FleetView, String> {
        let _transition = self.transition.lock().await;
        if self.data()?.hub.is_none() {
            return Err("This instance is not the coordinator".into());
        }
        self.change(|d| {
            d.hub = None;
            Ok(())
        })?;
        self.stop_runtime();
        self.set_error(None);
        self.local_view().await
    }
    pub async fn invite(&self, request: InviteRequest) -> Result<Invitation, String> {
        if !(30..=3600).contains(&request.expires_in_seconds) {
            return Err("Invitation lifetime must be 30–3600 seconds".into());
        }
        let code = secret("invite");
        let expires = now() + request.expires_in_seconds;
        let invitation = self.change(|d| {
            let hub = d
                .hub
                .as_mut()
                .filter(|h| h.enabled)
                .ok_or("Create invitations on the coordinator PC")?;
            hub.invitations.retain(|i| i.expires_at > now());
            if hub.invitations.len() >= 20 {
                return Err("Too many unused invitations; wait for expiry".into());
            }
            hub.invitations.push(PendingInvitation {
                hash: hash(&code),
                expires_at: expires,
            });
            Ok(InvitationKey {
                version: PROTOCOL_VERSION,
                fleet_id: hub.fleet_id.clone(),
                address: hub.address.clone(),
                certificate_pem: hub.certificate_pem.clone(),
                certificate_fingerprint: hub.fingerprint.clone(),
                code: code.clone(),
                expires_at: expires,
            })
        })?;
        let key = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&invitation).map_err(|e| e.to_string())?);
        Ok(Invitation {
            address: invitation.address,
            key,
            expires_at: expires,
            certificate_fingerprint: invitation.certificate_fingerprint,
        })
    }
    pub async fn join(self: &Arc<Self>, request: JoinRequest) -> Result<FleetView, String> {
        let _transition = self.transition.lock().await;
        let data = self.data()?;
        if data.member.is_some() || data.hub.is_some() {
            return Err("Leave or disable the existing Fleet first".into());
        }
        let name = if request.name.trim().is_empty() {
            local_name()
        } else {
            request.name.trim().to_owned()
        };
        if !valid_name(&name) || request.key.len() > 12000 {
            return Err("Invalid rig name or invitation".into());
        }
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(request.key.trim())
            .map_err(|_| "Invalid pairing key")?;
        let key: InvitationKey =
            serde_json::from_slice(&decoded).map_err(|_| "Invalid pairing key")?;
        let address = transport::normalize_address(&request.address)?;
        transport::normalize_address(&key.address)?;
        if key.version != PROTOCOL_VERSION
            || key.expires_at <= now()
            || key.code.len() > 128
            || transport::fingerprint(&key.certificate_pem)? != key.certificate_fingerprint
        {
            return Err("Expired invitation or invalid TLS trust information".into());
        }
        let client = transport::pinned_client(&key.certificate_pem)?;
        let response=client.post(format!("{address}/fleet/v1/enroll")).json(&EnrollRequest{code:key.code,rig_id:data.rig_id,name:name.clone()}).send().await.map_err(|_|"Cannot securely connect to the coordinator; verify its address and invitation certificate".to_string())?;
        let enrolled: EnrollResponse = transport::response(response).await?;
        if enrolled.fleet_id != key.fleet_id
            || enrolled.agent_token.len() > 128
            || enrolled.controller_token.len() > 128
            || same(&enrolled.agent_token, &enrolled.controller_token)
        {
            return Err("Invalid coordinator enrollment response".into());
        }
        self.change(|d| {
            d.name = name;
            for old in d.executed.values_mut() {
                old.delivered = true;
            }
            d.member = Some(Membership {
                fleet_id: key.fleet_id,
                address,
                certificate_pem: key.certificate_pem,
                fingerprint: key.certificate_fingerprint,
                agent_token: enrolled.agent_token,
                controller_token: enrolled.controller_token,
            });
            Ok(())
        })?;
        self.resume_runtime().await?;
        self.set_error(None);
        // Do not wait for the first polling cycle before returning usable UI.
        self.view().await
    }
    pub async fn leave(self: &Arc<Self>) -> Result<FleetView, String> {
        let _transition = self.transition.lock().await;
        let data = self.data()?;
        if data.hub.is_some() {
            return Err("Disable the coordinator instead of leaving it".into());
        }
        let mut warning = None;
        if let Some(member) = data.member {
            let revoke = async {
                let response = transport::pinned_client(&member.certificate_pem)?
                    .post(format!("{}/fleet/v1/leave", member.address))
                    .bearer_auth(&member.agent_token)
                    .send()
                    .await
                    .map_err(|_| "Coordinator unreachable".to_string())?;
                transport::response::<Value>(response).await
            }
            .await;
            if revoke.is_err() {
                warning = Some("Local pairing removed. Server revocation was not confirmed; remove this rig on the coordinator to invalidate any old credential copies.".into());
            }
        }
        self.change(|d| {
            d.member = None;
            Ok(())
        })?;
        self.stop_runtime();
        self.set_error(warning);
        if let Ok(mut cached) = self.cached.lock() {
            *cached = None;
        }
        self.local_view().await
    }
    pub async fn discover(&self) -> Result<Vec<DiscoveredHub>, String> {
        discovery::discover().await
    }
    async fn capture(&self) -> Result<(Value, Value), String> {
        let core = self.core.clone();
        tokio::task::spawn_blocking(move || Ok((core.snapshot()?, core.config()?)))
            .await
            .map_err(|_| "Fleet snapshot worker failed".to_string())?
    }
    async fn local_view(&self) -> Result<FleetView, String> {
        let (rig_id, name, mut commands) = {
            let store = self.store.lock().map_err(|_| lock_error())?;
            (
                store.data.rig_id.clone(),
                store.data.name.clone(),
                store
                    .data
                    .executed
                    .values()
                    .map(|e| e.receipt.clone())
                    .collect::<Vec<_>>(),
            )
        };
        let (snapshot, config) = self.capture().await?;
        commands.sort_by_key(|c| std::cmp::Reverse(c.created_at));
        commands.truncate(200);
        Ok(FleetView {
            setup: self.local_status(),
            rigs: vec![RigView {
                rig_id,
                name,
                last_seen: now(),
                online: true,
                snapshot,
                config,
            }],
            commands,
            coordinator_rig_id: None,
            rollouts: vec![],
            local_setup_allowed: false,
            controller_allowed: false,
        })
    }
    pub(crate) async fn hub_view(&self) -> Result<FleetView, String> {
        let data = self.data()?;
        let hub = data
            .hub
            .filter(|h| h.enabled)
            .ok_or("Coordinator is disabled")?;
        let mut view = self.local_view().await?;
        view.coordinator_rig_id = Some(data.rig_id);
        view.rollouts = hub.rollouts.values().map(|r| r.view.clone()).collect();
        view.rollouts.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        for rig in hub.rigs.into_values() {
            view.rigs.push(RigView {
                rig_id: rig.rig_id,
                name: rig.name,
                last_seen: rig.last_seen,
                online: rig.last_seen > 0 && now().saturating_sub(rig.last_seen) <= ONLINE_SECONDS,
                snapshot: rig.snapshot,
                config: rig.config,
            });
        }
        view.commands = hub
            .commands
            .into_values()
            .map(|mut c| {
                if !c.receipt.terminal() && c.command.expires_at <= now() {
                    c.receipt.state = "expired".into();
                    c.receipt.error = Some("Command expired before confirmation".into());
                }
                c.receipt
            })
            .collect();
        view.commands
            .sort_by_key(|c| std::cmp::Reverse(c.created_at));
        view.commands.truncate(200);
        Ok(view)
    }
    pub async fn view(&self) -> Result<FleetView, String> {
        let (hub, member) = {
            let store = self.store.lock().map_err(|_| lock_error())?;
            (store.data.hub.is_some(), store.data.member.clone())
        };
        if hub {
            return self.hub_view().await;
        }
        let Some(member) = member else {
            return self.local_view().await;
        };
        let result = async {
            let response = transport::pinned_client(&member.certificate_pem)?
                .get(format!("{}/fleet/v1/view", member.address))
                .bearer_auth(&member.controller_token)
                .send()
                .await
                .map_err(|_| {
                    "Coordinator is unreachable or its pinned TLS identity changed".to_string()
                })?;
            transport::response::<FleetView>(response).await
        }
        .await;
        match result {
            Ok(mut view) => {
                view.setup = self.local_status();
                view.local_setup_allowed = false;
                view.controller_allowed = false;
                if let Ok(mut cached) = self.cached.lock() {
                    *cached = Some(view.clone());
                }
                Ok(view)
            }
            Err(error) => {
                self.set_error(Some(error));
                let cached = self.cached.lock().ok().and_then(|c| c.clone());
                if let Some(mut view) = cached {
                    view.setup = self.local_status();
                    for rig in &mut view.rigs {
                        rig.online = false;
                    }
                    Ok(view)
                } else {
                    self.local_view().await
                }
            }
        }
    }
    pub async fn rig(&self, id: &str) -> Result<RigView, String> {
        self.view()
            .await?
            .rigs
            .into_iter()
            .find(|r| r.rig_id == id)
            .ok_or_else(|| "Rig not found".into())
    }
    pub async fn rig_config(&self, id: &str) -> Result<Value, String> {
        Ok(self.rig(id).await?.config)
    }
    pub async fn receipt(&self, id: &str) -> Result<CommandReceipt, String> {
        self.view()
            .await?
            .commands
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| "Command receipt not found".into())
    }
    pub async fn command(&self, request: CommandRequest) -> Result<Vec<CommandReceipt>, String> {
        if matches!(request.operation, FleetOperation::UpdateInstall { .. }) {
            return Err("Use a confirmed Fleet update rollout to install software".into());
        }
        let data = self.data()?;
        if let Some(member) = data.member {
            let response = transport::pinned_client(&member.certificate_pem)?
                .post(format!("{}/fleet/v1/commands", member.address))
                .bearer_auth(member.controller_token)
                .json(&request)
                .send()
                .await
                .map_err(|_| "Coordinator is unreachable; no command was confirmed".to_string())?;
            transport::response(response).await
        } else {
            self.queue_commands(request).await
        }
    }
    pub(crate) async fn queue_commands(
        &self,
        request: CommandRequest,
    ) -> Result<Vec<CommandReceipt>, String> {
        if request.rig_ids.is_empty() || request.rig_ids.len() > 100 {
            return Err("Select between 1 and 100 rigs".into());
        }
        if serde_json::to_vec(&request)
            .map_err(|e| e.to_string())?
            .len()
            > 128 * 1024
        {
            return Err("Fleet command exceeds the size limit".into());
        }
        let mut ids = request.rig_ids.clone();
        ids.sort();
        ids.dedup();
        if ids.len() != request.rig_ids.len() {
            return Err("Duplicate rig IDs".into());
        }
        let lifetime = request
            .expires_in_seconds
            .unwrap_or(DEFAULT_COMMAND_SECONDS);
        if !(5..=300).contains(&lifetime) {
            return Err("Command expiry must be 5–300 seconds".into());
        }
        let data = self.data()?;
        let local = data.rig_id.clone();
        let mut commands = vec![];
        for id in ids {
            let mut command = FleetCommand {
                id: uuid::Uuid::new_v4().to_string(),
                rig_id: id.clone(),
                created_at: now(),
                expires_at: now() + lifetime,
                operation: request.operation.clone(),
            };
            if matches!(
                command.operation,
                FleetOperation::Start { .. }
                    | FleetOperation::ForceStart { .. }
                    | FleetOperation::Restart { .. }
                    | FleetOperation::Resume { .. }
            ) {
                command.expires_at = command.expires_at.min(now() + 60);
            }
            let mut receipt = CommandReceipt::queued(&command);
            let online = id == local
                || data
                    .hub
                    .as_ref()
                    .and_then(|h| h.rigs.get(&id))
                    .is_some_and(|r| {
                        r.last_seen > 0 && now().saturating_sub(r.last_seen) <= ONLINE_SECONDS
                    });
            if !online {
                receipt.state = "failed".into();
                receipt.error =
                    Some("Rig is offline or not enrolled; no command was queued".into());
            } else if matches!(command.operation, FleetOperation::UpdateCheck | FleetOperation::UpdateInstall { .. })
                && id != local
                && !data.hub.as_ref().and_then(|h| h.rigs.get(&id))
                    .and_then(|r| r.snapshot.pointer("/updates/protocol")).and_then(Value::as_u64).is_some_and(|v| v == 1)
            {
                receipt.state = "failed".into();
                receipt.error = Some("Upgrade this rig locally once to enable Fleet software updates".into());
            }
            commands.push(StoredCommand { command, receipt });
        }
        if data.hub.is_some() {
            self.change(|d| {
                let hub = d
                    .hub
                    .as_mut()
                    .filter(|h| h.enabled)
                    .ok_or("Coordinator is disabled")?;
                // Keep active rollout acknowledgements while pruning ordinary
                // terminal history and expiring abandoned offline commands.
                rollouts::prune_commands(hub);
                if hub.commands.len() + commands.len() > 640 {
                    return Err(
                        "Fleet command history is full; wait for pending commands to finish".into(),
                    );
                }
                for command in &commands {
                    hub.commands
                        .insert(command.command.id.clone(), command.clone());
                }
                Ok(())
            })?;
        }
        let mut receipts = vec![];
        for stored in commands {
            if stored.command.rig_id == local && !stored.receipt.terminal() {
                let receipt = self.execute_once(&stored.command).await?;
                self.record_hub_result(&receipt)?;
                receipts.push(receipt);
            } else {
                receipts.push(stored.receipt);
            }
        }
        Ok(receipts)
    }
    fn record_hub_result(&self, receipt: &CommandReceipt) -> Result<(), String> {
        if self
            .store
            .lock()
            .map_err(|_| lock_error())?
            .data
            .hub
            .is_none()
        {
            return Ok(());
        }
        self.change(|d| {
            if let Some(stored) = d.hub.as_mut().and_then(|h| h.commands.get_mut(&receipt.id)) {
                stored.receipt = receipt.clone();
            }
            Ok(())
        })
    }
    pub async fn revoke(&self, id: &str) -> Result<FleetView, String> {
        self.change(|d| {
            let hub = d
                .hub
                .as_mut()
                .filter(|h| h.enabled)
                .ok_or("Revoke rig access on the coordinator PC")?;
            if hub.rigs.remove(id).is_none() {
                return Err("Enrolled rig not found".into());
            }
            for stored in hub
                .commands
                .values_mut()
                .filter(|c| c.command.rig_id == id && !c.receipt.terminal())
            {
                stored.receipt.state = "failed".into();
                stored.receipt.error = Some("Rig access was revoked".into());
            }
            Ok(())
        })?;
        self.hub_view().await
    }
    // A machine can revoke only the registration selected by its own credential.
    // This TLS endpoint accepts no caller-supplied target ID.
    pub(crate) fn revoke_self(&self, token: &str) -> Result<(), String> {
        let digest = hash(token);
        self.change(|data| {
            let hub = data
                .hub
                .as_mut()
                .filter(|h| h.enabled)
                .ok_or("Coordinator is disabled")?;
            let id = hub
                .rigs
                .values()
                .find(|rig| same(&rig.agent_hash, &digest) || same(&rig.controller_hash, &digest))
                .map(|rig| rig.rig_id.clone())
                .ok_or("Fleet credential is invalid or already revoked")?;
            hub.rigs.remove(&id);
            for stored in hub
                .commands
                .values_mut()
                .filter(|command| command.command.rig_id == id && !command.receipt.terminal())
            {
                stored.receipt.state = "failed".into();
                stored.receipt.error = Some("Rig left the fleet; its access was revoked".into());
            }
            Ok(())
        })
    }
    pub(crate) fn authorize_controller(&self, token: &str) -> Result<(), String> {
        let digest = hash(token);
        let store = self.store.lock().map_err(|_| lock_error())?;
        let hub = store
            .data
            .hub
            .as_ref()
            .filter(|h| h.enabled)
            .ok_or("Coordinator is disabled")?;
        if hub.rigs.values().any(|r| same(&r.controller_hash, &digest)) {
            Ok(())
        } else {
            Err("Controller credential is invalid or revoked".into())
        }
    }
    pub(crate) fn enroll(&self, request: EnrollRequest) -> Result<EnrollResponse, String> {
        if !valid_name(&request.name)
            || uuid::Uuid::parse_str(&request.rig_id).is_err()
            || request.code.len() > 128
        {
            return Err("Invalid enrollment request".into());
        }
        let digest = hash(&request.code);
        {
            let store = self.store.lock().map_err(|_| lock_error())?;
            let hub = store
                .data
                .hub
                .as_ref()
                .filter(|h| h.enabled)
                .ok_or("Coordinator is disabled")?;
            if !hub
                .invitations
                .iter()
                .any(|i| i.expires_at > now() && same(&i.hash, &digest))
            {
                return Err("Invitation is expired, already used or invalid".into());
            }
        }
        let agent = secret("agent");
        let controller = secret("controller");
        self.change(|d| {
            if d.rig_id == request.rig_id {
                return Err("The coordinator cannot enroll itself".into());
            }
            let hub = d
                .hub
                .as_mut()
                .filter(|h| h.enabled)
                .ok_or("Coordinator is disabled")?;
            let index = hub
                .invitations
                .iter()
                .position(|i| i.expires_at > now() && same(&i.hash, &digest))
                .ok_or("Invitation is expired, already used or invalid")?;
            if hub.rigs.contains_key(&request.rig_id) {
                return Err("This rig identity is already enrolled".into());
            }
            if hub.rigs.len() >= 100 {
                return Err("Fleet supports up to 100 enrolled rigs".into());
            }
            hub.invitations.remove(index);
            hub.rigs.insert(
                request.rig_id.clone(),
                RegisteredRig {
                    rig_id: request.rig_id,
                    name: request.name,
                    agent_hash: hash(&agent),
                    controller_hash: hash(&controller),
                    last_seen: 0,
                    snapshot: Value::Null,
                    config: Value::Null,
                },
            );
            Ok(EnrollResponse {
                fleet_id: hub.fleet_id.clone(),
                agent_token: agent.clone(),
                controller_token: controller.clone(),
            })
        })
    }
    pub(crate) fn receive_poll(
        &self,
        token: &str,
        mut request: PollRequest,
    ) -> Result<PollResponse, String> {
        let digest = hash(token);
        if request.results.len() > 256
            || !request.snapshot.is_object()
            || !request.config.is_object()
        {
            return Err("Invalid agent poll".into());
        }
        if serde_json::to_vec(&(&request.snapshot, &request.config))
            .map_err(|e| e.to_string())?
            .len()
            > 96 * 1024
        {
            return Err("Rig snapshot/config exceeds the 96 KiB Fleet limit".into());
        }
        request.snapshot = validation::snapshot(request.snapshot)?;
        request.config = validation::config(request.config)?;
        let mut store = self.store.lock().map_err(|_| lock_error())?;
        let hub = store
            .data
            .hub
            .as_ref()
            .filter(|h| h.enabled)
            .ok_or("Coordinator is disabled")?;
        let id = hub
            .rigs
            .values()
            .find(|r| same(&r.agent_hash, &digest))
            .map(|r| r.rig_id.clone())
            .ok_or("Agent credential is invalid or revoked")?;
        let durable = request.results.iter().any(|r| {
            r.rig_id == id
                && hub
                    .commands
                    .get(&r.id)
                    .is_some_and(|c| c.command.rig_id == id && !c.receipt.terminal())
        }) || hub.commands.values().any(|c| {
            c.command.rig_id == id && !c.receipt.terminal() && c.command.expires_at <= now()
        });
        if durable {
            store.transaction(|data| Self::apply_poll(data, &id, request))
        } else {
            Self::apply_poll(&mut store.data, &id, request)
        }
    }
    fn apply_poll(
        data: &mut Persistent,
        id: &str,
        request: PollRequest,
    ) -> Result<PollResponse, String> {
        let hub = data
            .hub
            .as_mut()
            .filter(|h| h.enabled)
            .ok_or("Coordinator is disabled")?;
        let rig = hub.rigs.get_mut(id).ok_or("Rig enrollment is missing")?;
        rig.last_seen = now();
        rig.snapshot = request.snapshot;
        rig.config = request.config;
        let mut acknowledged = vec![];
        for receipt in request.results {
            if receipt.rig_id != id
                || !matches!(
                    receipt.state.as_str(),
                    "running" | "succeeded" | "failed" | "expired"
                )
                || serde_json::to_vec(&receipt)
                    .map(|r| r.len() > 16 * 1024)
                    .unwrap_or(true)
            {
                continue;
            }
            if let Some(stored) = hub.commands.get_mut(&receipt.id) {
                if stored.command.rig_id == id {
                    acknowledged.push(receipt.id.clone());
                    if !stored.receipt.terminal() {
                        stored.receipt.state = receipt.state;
                        stored.receipt.result = receipt.result;
                        stored.receipt.error = receipt.error;
                    }
                }
            } else if receipt.expires_at <= now() {
                acknowledged.push(receipt.id);
            }
        }
        let mut commands = vec![];
        for stored in hub
            .commands
            .values_mut()
            .filter(|c| c.command.rig_id == id && !c.receipt.terminal())
        {
            if stored.command.expires_at <= now() {
                stored.receipt.state = "expired".into();
                stored.receipt.error = Some("Command expired before agent execution".into());
            } else if commands.len() < 16 {
                commands.push(stored.command.clone());
            }
        }
        Ok(PollResponse {
            commands,
            acknowledged_results: acknowledged,
        })
    }
    async fn execute_once(&self, command: &FleetCommand) -> Result<CommandReceipt, String> {
        let id = self.data()?.rig_id;
        if command.rig_id != id {
            return Err("Command targets another rig".into());
        }
        let fingerprint = hash(&serde_json::to_string(command).map_err(|e| e.to_string())?);
        let mut receipt = CommandReceipt::queued(command);
        let existing = self.change(|d| {
            if let Some(old) = d.executed.get(&command.id) {
                if old.fingerprint != fingerprint {
                    return Err("Command ID was reused with different contents".into());
                }
                return Ok(Some(old.receipt.clone()));
            }
            if command.expires_at <= now()
                || command.created_at > now() + 30
                || command.expires_at.saturating_sub(command.created_at) > 300
            {
                receipt.state = "expired".into();
                receipt.error = Some("Expired or invalid command was not executed".into());
            } else {
                receipt.state = "running".into();
            }
            if d.executed.len() > 512 {
                let keys: Vec<_> = d
                    .executed
                    .iter()
                    .filter(|(_, e)| e.receipt.terminal() && e.receipt.expires_at < now())
                    .map(|(id, _)| id.clone())
                    .take(128)
                    .collect();
                for key in keys {
                    d.executed.remove(&key);
                }
            }
            if d.executed.len() >= 1024 {
                return Err(
                    "Local command journal is full; waiting for old commands to expire".into(),
                );
            }
            d.executed.insert(
                command.id.clone(),
                Executed {
                    fingerprint: fingerprint.clone(),
                    receipt: receipt.clone(),
                    delivered: false,
                },
            );
            Ok(None)
        })?;
        if let Some(receipt) = existing {
            return Ok(receipt);
        }
        if receipt.terminal() {
            return Ok(receipt);
        }
        let core = self.core.clone();
        let operation = command.operation.clone();
        let deadline = command.expires_at;
        let outcome = tokio::task::spawn_blocking(move || {
            if now() >= deadline {
                return Err("Command expired while waiting for the local worker".into());
            }
            core.execute(&operation, deadline)
        })
        .await
        .map_err(|_| "Local command worker failed".to_string())
        .and_then(|r| r);
        match outcome {
            Ok(result) => {
                receipt.state = "succeeded".into();
                receipt.result = Some(
                    if serde_json::to_vec(&result)
                        .map(|r| r.len() > 8 * 1024)
                        .unwrap_or(true)
                    {
                        json!({"truncated":true,"message":"Command completed; its large result was omitted from the bounded Fleet journal"})
                    } else {
                        result
                    },
                );
            }
            Err(error) => {
                receipt.state = "failed".into();
                receipt.error = Some(error.chars().take(2000).collect());
            }
        }
        self.change(|d| {
            d.executed
                .get_mut(&command.id)
                .ok_or("Missing command journal")?
                .receipt = receipt.clone();
            Ok(())
        })?;
        Ok(receipt)
    }
    async fn agent_loop(self: Arc<Self>, mut stop: watch::Receiver<bool>) {
        loop {
            if *stop.borrow() {
                return;
            }
            match self.poll_once().await {
                Ok(()) => self.set_error(None),
                Err(error) => self.set_error(Some(error)),
            }
            tokio::select! {_=stop.changed()=>return,_=tokio::time::sleep(Duration::from_secs(3))=>{}}
        }
    }
    async fn poll_once(&self) -> Result<(), String> {
        let data = self.data()?;
        let member = data.member.ok_or("Rig has left the Fleet")?;
        let (snapshot, config) = self.capture().await?;
        let results = data
            .executed
            .values()
            .filter(|e| !e.delivered)
            .take(256)
            .map(|e| e.receipt.clone())
            .collect();
        let client = transport::pinned_client(&member.certificate_pem)?;
        let response = client
            .post(format!("{}/fleet/v1/poll", member.address))
            .bearer_auth(&member.agent_token)
            .json(&PollRequest {
                snapshot,
                config,
                results,
            })
            .send()
            .await
            .map_err(|_| {
                "Agent cannot securely reach the coordinator; local schedules continue".to_string()
            })?;
        let response: PollResponse = transport::response(response).await?;
        // Leaving/re-pairing while a poll was in flight must not execute old work.
        if !self
            .data()?
            .member
            .is_some_and(|m| same(&m.agent_token, &member.agent_token))
        {
            return Ok(());
        }
        if !response.acknowledged_results.is_empty() {
            self.change(|d| {
                for id in response.acknowledged_results.iter().take(256) {
                    if let Some(execution) = d.executed.get_mut(id) {
                        execution.delivered = true;
                    }
                }
                Ok(())
            })?;
        }
        for command in response.commands.into_iter().take(16) {
            if !self
                .data()?
                .member
                .is_some_and(|m| same(&m.agent_token, &member.agent_token))
            {
                return Ok(());
            }
            self.execute_once(&command).await?;
        }
        Ok(())
    }
    fn lan_address(&self, hub: &HubRecord) -> String {
        let Ok(bind) = hub.listen.parse::<SocketAddr>() else {
            return hub.address.clone();
        };
        let candidate = if bind.ip().is_unspecified() {
            self.suggested.clone()
        } else {
            format!("https://{}", bind)
        };
        reqwest::Url::parse(&candidate)
            .ok()
            .and_then(|mut url| {
                url.set_port(Some(bind.port())).ok()?;
                Some(url.as_str().trim_end_matches('/').to_string())
            })
            .unwrap_or_else(|| hub.address.clone())
    }
}
fn suggested_url() -> String {
    // UDP connect selects an existing local route; it sends no packet and does
    // not contact the documentation-only destination 192.0.2.1.
    let address = std::net::UdpSocket::bind("0.0.0.0:0")
        .ok()
        .and_then(|socket| {
            socket.connect("192.0.2.1:9").ok()?;
            socket.local_addr().ok()
        })
        .map(|a| a.ip())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
    address
        .map(|ip| format!("https://{ip}:17889"))
        .unwrap_or_else(|| "https://localhost:17889".into())
}
