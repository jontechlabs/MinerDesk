//! Versioned, allowlisted Fleet messages. No arbitrary shell or HTTP forwarding.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;
pub const ONLINE_SECONDS: u64 = 20;
pub const DEFAULT_COMMAND_SECONDS: u64 = 90;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum FleetOperation {
    Start {
        profile_id: String,
    },
    Stop {
        profile_id: String,
    },
    Restart {
        profile_id: String,
    },
    Resume {
        profile_id: String,
    },
    ForceStart {
        profile_id: String,
    },
    ProfilePatch {
        profile_id: String,
        expected_revision: String,
        patch: Value,
    },
    SchedulesPatch {
        expected_revision: String,
        schedules: Vec<crate::ScheduleWindow>,
    },
    Logs {
        profile_id: String,
        limit: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandRequest {
    pub rig_ids: Vec<String>,
    pub operation: FleetOperation,
    #[serde(default)]
    pub expires_in_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetCommand {
    pub id: String,
    pub rig_id: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub operation: FleetOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub id: String,
    pub rig_id: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub state: String,
    pub result: Option<Value>,
    pub error: Option<String>,
}

impl CommandReceipt {
    pub fn queued(command: &FleetCommand) -> Self {
        Self {
            id: command.id.clone(),
            rig_id: command.rig_id.clone(),
            created_at: command.created_at,
            expires_at: command.expires_at,
            state: "queued".into(),
            result: None,
            error: None,
        }
    }
    pub fn terminal(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "failed" | "expired")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetSetup {
    pub rig_id: String,
    pub name: String,
    pub mode: String,
    pub hub_url: Option<String>,
    pub fleet_id: Option<String>,
    pub hub_enabled: bool,
    pub tls_fingerprint: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub suggested_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigView {
    pub rig_id: String,
    pub name: String,
    pub last_seen: u64,
    pub online: bool,
    pub snapshot: Value,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetView {
    pub setup: FleetSetup,
    pub rigs: Vec<RigView>,
    pub commands: Vec<CommandReceipt>,
    #[serde(default)]
    pub local_setup_allowed: bool,
    #[serde(default)]
    pub controller_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateHub {
    pub name: String,
    pub advertise_url: String,
    #[serde(default = "default_listen")]
    pub listen: String,
}
fn default_listen() -> String {
    "0.0.0.0:17889".into()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinRequest {
    pub address: String,
    pub key: String,
    // Older clients may still supply a label; new clients use the machine name.
    #[serde(default)]
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InviteRequest {
    #[serde(default = "default_invite_seconds")]
    pub expires_in_seconds: u64,
}
fn default_invite_seconds() -> u64 {
    300
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invitation {
    pub address: String,
    pub key: String,
    pub expires_at: u64,
    pub certificate_fingerprint: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredHub {
    pub fleet_id: String,
    pub name: String,
    pub address: String,
    pub certificate_fingerprint: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct InvitationKey {
    pub version: u32,
    pub fleet_id: String,
    pub address: String,
    pub certificate_pem: String,
    pub certificate_fingerprint: String,
    pub code: String,
    pub expires_at: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnrollRequest {
    pub code: String,
    pub rig_id: String,
    pub name: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct EnrollResponse {
    pub fleet_id: String,
    pub agent_token: String,
    pub controller_token: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PollRequest {
    pub snapshot: Value,
    pub config: Value,
    pub results: Vec<CommandReceipt>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PollResponse {
    pub commands: Vec<FleetCommand>,
    #[serde(default)]
    pub acknowledged_results: Vec<String>,
}
