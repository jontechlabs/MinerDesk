//! Durable, sequential update intent. A successful command means accepted work;
//! only a fresh report of the target binary confirms an installation.
use super::*;

const CONFIRM_SECONDS: u64 = 45 * 60;
fn active_item(state: &str) -> bool {
    matches!(state, "installing" | "waiting_for_reconnect")
}
fn active_plan(plan: &UpdateRollout) -> bool {
    matches!(plan.state.as_str(), "running" | "waiting_for_reconnect")
        || plan.rigs.iter().any(|r| active_item(&r.state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    const TARGET: &str = "9.9.9";
    struct DemoCore {
        calls: AtomicUsize,
    }
    fn snapshot(version: &str, job: Value) -> Value {
        json!({"health":"online","name":"fixture","os":"linux","arch":"x86_64","version":version,
            "local_time":"2026-11-12T12:00:00Z","timezone":"+00:00","utc_offset_seconds":0,
            "status":[],"revision":"1","gpu_inventory":[],"gpu_inventory_available":false,
            "updates":{"protocol":1,"current_version":version,"available_version":TARGET,
                "enabled":true,"can_install":true,"reason":"supported","root_required":false,"root_enrolled":false,"job":job}})
    }
    impl FleetCore for DemoCore {
        fn snapshot(&self) -> Result<Value, String> {
            Ok(snapshot("0.8.2", Value::Null))
        }
        fn config(&self) -> Result<Value, String> {
            Ok(json!({"revision":"1","profiles":[],"schedules":[],
            "local_time":"2026-11-12T12:00:00Z","timezone":"+00:00","utc_offset_seconds":0,"editable_profile_fields":[]}))
        }
        fn execute(&self, _: &FleetOperation, _: u64) -> Result<Value, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"job":{"id":uuid::Uuid::new_v4().to_string()}}))
        }
    }
    async fn setup() -> (tempfile::TempDir, Arc<FleetManager>, Arc<DemoCore>, String) {
        let dir = tempfile::tempdir().unwrap();
        let core = Arc::new(DemoCore {
            calls: AtomicUsize::new(0),
        });
        let manager = FleetManager::new(dir.path().join("config.json"), core.clone()).unwrap();
        manager
            .create(CreateHub {
                name: "test coordinator".into(),
                advertise_url: "https://127.0.0.1:0".into(),
                listen: "127.0.0.1:0".into(),
            })
            .await
            .unwrap();
        let peer = uuid::Uuid::new_v4().to_string();
        manager
            .change(|data| {
                data.hub.as_mut().unwrap().rigs.insert(
                    peer.clone(),
                    RegisteredRig {
                        rig_id: peer.clone(),
                        name: "test peer".into(),
                        agent_hash: "fixture".into(),
                        controller_hash: "fixture".into(),
                        last_seen: now(),
                        snapshot: snapshot("0.8.2", Value::Null),
                        config: core.config().unwrap(),
                    },
                );
                Ok(())
            })
            .unwrap();
        (dir, manager, core, peer)
    }
    fn plan(manager: &FleetManager, id: &str) -> StoredRollout {
        manager.data().unwrap().hub.unwrap().rollouts[id].clone()
    }
    fn report(manager: &FleetManager, peer: &str, version: &str, job: Value) {
        manager
            .change(|data| {
                let peer = data.hub.as_mut().unwrap().rigs.get_mut(peer).unwrap();
                peer.snapshot = snapshot(version, job);
                peer.last_seen = now();
                Ok(())
            })
            .unwrap();
    }
    fn report_completed(manager: &FleetManager, peer: &str) {
        let data = manager.data().unwrap();
        let id = data
            .hub
            .unwrap()
            .commands
            .values()
            .find(|c| c.command.rig_id == peer)
            .and_then(|c| c.receipt.result.as_ref())
            .and_then(|r| r.pointer("/job/id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        report(
            manager,
            peer,
            TARGET,
            json!({"id":id,"expected_version":TARGET,"stage":"completed","error":null}),
        );
    }
    #[tokio::test]
    async fn coordinator_last_waits_for_reported_version_not_command_success() {
        let (_dir, manager, core, peer) = setup().await;
        let own = manager.data().unwrap().rig_id;
        let rollout = manager
            .create_rollout(UpdateRolloutRequest {
                rig_ids: vec![own.clone(), peer.clone()],
                expected_version: TARGET.into(),
            })
            .await
            .unwrap();
        assert_eq!(
            rollout
                .rigs
                .iter()
                .map(|r| r.rig_id.clone())
                .collect::<Vec<_>>(),
            vec![peer.clone(), own]
        );
        manager.tick_rollouts().await.unwrap();
        let id = plan(&manager, &rollout.id).command_id.unwrap();
        manager
            .change(|data| {
                let c = data.hub.as_mut().unwrap().commands.get_mut(&id).unwrap();
                c.receipt.state = "succeeded".into();
                c.receipt.result = Some(json!({"job":{"id":uuid::Uuid::new_v4().to_string()}}));
                Ok(())
            })
            .unwrap();
        manager.tick_rollouts().await.unwrap();
        assert_eq!(plan(&manager, &rollout.id).view.rigs[0].state, "installing");
        assert_eq!(core.calls.load(Ordering::SeqCst), 0);
        report_completed(&manager, &peer);
        manager.tick_rollouts().await.unwrap();
        assert_eq!(plan(&manager, &rollout.id).view.rigs[0].state, "updated");
        manager.tick_rollouts().await.unwrap(); // durable coordinator intent
        manager.tick_rollouts().await.unwrap(); // accepted locally, not installed
        assert_eq!(core.calls.load(Ordering::SeqCst), 1);
        assert_ne!(plan(&manager, &rollout.id).view.state, "completed");
        manager.stop_runtime();
    }
    #[tokio::test]
    async fn cancellation_keeps_inflight_work_but_never_starts_remaining_targets() {
        let (_dir, manager, core, peer) = setup().await;
        let rollout = manager
            .create_rollout(UpdateRolloutRequest {
                rig_ids: vec![manager.data().unwrap().rig_id, peer.clone()],
                expected_version: TARGET.into(),
            })
            .await
            .unwrap();
        manager.tick_rollouts().await.unwrap();
        let cancelled = manager.cancel_rollout(&rollout.id).await.unwrap();
        assert_eq!(cancelled.state, "cancelled");
        assert_eq!(cancelled.rigs[1].state, "cancelled");
        report_completed(&manager, &peer);
        manager.tick_rollouts().await.unwrap();
        manager.tick_rollouts().await.unwrap();
        let current = plan(&manager, &rollout.id);
        assert_eq!(current.view.state, "cancelled");
        assert_eq!(current.view.rigs[0].state, "updated");
        assert_eq!(core.calls.load(Ordering::SeqCst), 0);
        assert_eq!(manager.data().unwrap().hub.unwrap().commands.len(), 1);
        manager.stop_runtime();
    }
    #[tokio::test]
    async fn durable_intent_survives_restart_without_creating_duplicate_commands() {
        let (dir, manager, core, peer) = setup().await;
        let rollout = manager
            .create_rollout(UpdateRolloutRequest {
                rig_ids: vec![peer.clone()],
                expected_version: TARGET.into(),
            })
            .await
            .unwrap();
        manager.tick_rollouts().await.unwrap();
        let id = plan(&manager, &rollout.id).command_id;
        let restored = FleetManager::new(dir.path().join("config.json"), core).unwrap();
        report(&restored, &peer, "0.8.2", Value::Null); // fresh reconnection
        restored.tick_rollouts().await.unwrap();
        assert_eq!(plan(&restored, &rollout.id).command_id, id);
        assert_eq!(restored.data().unwrap().hub.unwrap().commands.len(), 1);
        manager.stop_runtime();
    }
    #[tokio::test]
    async fn a_previous_failed_job_cannot_fail_a_new_retry_for_the_same_release() {
        let (_dir, manager, _core, peer) = setup().await;
        let old = uuid::Uuid::new_v4().to_string();
        let fresh = uuid::Uuid::new_v4().to_string();
        report(
            &manager,
            &peer,
            "0.8.2",
            json!({"id":old,"expected_version":TARGET,"stage":"failed","error":"previous failure"}),
        );
        let rollout = manager
            .create_rollout(UpdateRolloutRequest {
                rig_ids: vec![peer.clone()],
                expected_version: TARGET.into(),
            })
            .await
            .unwrap();
        manager.tick_rollouts().await.unwrap();
        manager.tick_rollouts().await.unwrap();
        assert_eq!(plan(&manager, &rollout.id).view.state, "running");
        let id = plan(&manager, &rollout.id).command_id.unwrap();
        manager
            .change(|data| {
                let c = data.hub.as_mut().unwrap().commands.get_mut(&id).unwrap();
                c.receipt.state = "succeeded".into();
                c.receipt.result = Some(json!({"job":{"id":fresh}}));
                Ok(())
            })
            .unwrap();
        manager.tick_rollouts().await.unwrap();
        assert_eq!(plan(&manager, &rollout.id).view.state, "running");
        report(
            &manager,
            &peer,
            "0.8.2",
            json!({"id":fresh,"expected_version":TARGET,"stage":"failed","error":"current failure"}),
        );
        manager.tick_rollouts().await.unwrap();
        assert_eq!(plan(&manager, &rollout.id).view.state, "failed");
        manager.stop_runtime();
    }
    #[tokio::test]
    async fn history_is_bounded_and_preserves_an_active_rollouts_receipt() {
        let (_dir, manager, _core, peer) = setup().await;
        let rollout = manager
            .create_rollout(UpdateRolloutRequest {
                rig_ids: vec![peer],
                expected_version: TARGET.into(),
            })
            .await
            .unwrap();
        manager.tick_rollouts().await.unwrap();
        let protected = plan(&manager, &rollout.id).command_id.unwrap();
        manager
            .change(|data| {
                let hub = data.hub.as_mut().unwrap();
                hub.commands.get_mut(&protected).unwrap().receipt.state = "succeeded".into();
                for n in 0..700 {
                    let c = FleetCommand {
                        id: uuid::Uuid::new_v4().to_string(),
                        rig_id: data.rig_id.clone(),
                        created_at: n,
                        expires_at: now() + 60,
                        operation: FleetOperation::UpdateCheck,
                    };
                    let mut r = CommandReceipt::queued(&c);
                    r.state = "succeeded".into();
                    hub.commands.insert(
                        c.id.clone(),
                        StoredCommand {
                            command: c,
                            receipt: r,
                        },
                    );
                }
                prune_commands(hub);
                assert!(hub.commands.len() <= 384);
                assert!(hub.commands.contains_key(&protected));
                Ok(())
            })
            .unwrap();
        manager.stop_runtime();
    }
    #[tokio::test]
    async fn old_unapproved_and_offline_rigs_never_receive_installation_commands() {
        let (_dir, manager, core, peer) = setup().await;
        for value in [
            Value::Null,
            json!({"protocol":1,"enabled":false,"can_install":false}),
        ] {
            manager
                .change(|data| {
                    data.hub
                        .as_mut()
                        .unwrap()
                        .rigs
                        .get_mut(&peer)
                        .unwrap()
                        .snapshot["updates"] = value;
                    Ok(())
                })
                .unwrap();
            assert!(manager
                .create_rollout(UpdateRolloutRequest {
                    rig_ids: vec![peer.clone()],
                    expected_version: TARGET.into()
                })
                .await
                .is_err());
        }
        assert_eq!(manager.data().unwrap().hub.unwrap().commands.len(), 0);
        assert_eq!(core.calls.load(Ordering::SeqCst), 0);
        assert!(manager
            .command(CommandRequest {
                rig_ids: vec![peer],
                operation: FleetOperation::UpdateInstall {
                    expected_version: TARGET.into()
                },
                expires_in_seconds: None
            })
            .await
            .is_err());
        manager.stop_runtime();
    }
    #[tokio::test]
    async fn copied_new_binary_is_not_success_until_the_worker_completes() {
        let (_dir, manager, _core, peer) = setup().await;
        let rollout = manager
            .create_rollout(UpdateRolloutRequest {
                rig_ids: vec![peer.clone()],
                expected_version: TARGET.into(),
            })
            .await
            .unwrap();
        manager.tick_rollouts().await.unwrap();
        let command_id = plan(&manager, &rollout.id).command_id.unwrap();
        let job_id = uuid::Uuid::new_v4().to_string();
        manager
            .change(|data| {
                let c = data
                    .hub
                    .as_mut()
                    .unwrap()
                    .commands
                    .get_mut(&command_id)
                    .unwrap();
                c.receipt.state = "succeeded".into();
                c.receipt.result = Some(json!({"job":{"id":job_id}}));
                Ok(())
            })
            .unwrap();
        report(
            &manager,
            &peer,
            TARGET,
            json!({"id":job_id,"expected_version":TARGET,"stage":"restarting","error":null}),
        );
        manager.tick_rollouts().await.unwrap();
        assert_ne!(plan(&manager, &rollout.id).view.state, "completed");
        report(
            &manager,
            &peer,
            TARGET,
            json!({"id":job_id,"expected_version":TARGET,"stage":"failed","error":"installer failed after replacing executable"}),
        );
        manager.tick_rollouts().await.unwrap();
        assert_eq!(plan(&manager, &rollout.id).view.state, "failed");
        manager.stop_runtime();
    }
    #[tokio::test]
    async fn missing_confirmation_expires_without_starting_the_coordinator() {
        let (_dir, manager, core, peer) = setup().await;
        let rollout = manager
            .create_rollout(UpdateRolloutRequest {
                rig_ids: vec![peer, manager.data().unwrap().rig_id],
                expected_version: TARGET.into(),
            })
            .await
            .unwrap();
        manager.tick_rollouts().await.unwrap();
        manager
            .change(|data| {
                data.hub
                    .as_mut()
                    .unwrap()
                    .rollouts
                    .get_mut(&rollout.id)
                    .unwrap()
                    .deadline_at = now() - 1;
                Ok(())
            })
            .unwrap();
        manager.tick_rollouts().await.unwrap();
        assert_eq!(plan(&manager, &rollout.id).view.state, "failed");
        assert_eq!(plan(&manager, &rollout.id).view.rigs[1].state, "cancelled");
        assert_eq!(core.calls.load(Ordering::SeqCst), 0);
        manager.stop_runtime();
    }
}
pub(super) fn prune_commands(hub: &mut HubRecord) {
    for command in hub
        .commands
        .values_mut()
        .filter(|c| !c.receipt.terminal() && c.command.expires_at <= now())
    {
        command.receipt.state = "expired".into();
        command.receipt.error = Some("Command expired before agent confirmation".into());
    }
    if hub.commands.len() <= 512 {
        return;
    }
    let protected: std::collections::HashSet<_> = hub
        .rollouts
        .values()
        .filter(|p| active_plan(&p.view))
        .filter_map(|p| p.command_id.as_deref())
        .collect();
    let mut terminal: Vec<_> = hub
        .commands
        .iter()
        .filter(|(id, c)| c.receipt.terminal() && !protected.contains(id.as_str()))
        .map(|(id, c)| (id.clone(), c.command.created_at))
        .collect();
    terminal.sort_by_key(|c| c.1);
    for (id, _) in terminal
        .into_iter()
        .take(hub.commands.len().saturating_sub(384))
    {
        hub.commands.remove(&id);
    }
}
fn stable(value: &str) -> Result<semver::Version, String> {
    let parsed = semver::Version::parse(value).map_err(|_| "Invalid update version")?;
    if value.len() > 64
        || !parsed.pre.is_empty()
        || !parsed.build.is_empty()
        || parsed.to_string() != value
    {
        return Err("Choose an exact stable release version".into());
    }
    Ok(parsed)
}
fn target_reached(rig: &RigView, version: &str) -> bool {
    rig.online
        && rig.last_seen > 0
        && now().saturating_sub(rig.last_seen) <= ONLINE_SECONDS
        && stable(
            rig.snapshot
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )
        .ok()
        .zip(stable(version).ok())
        .is_some_and(|(current, target)| current >= target)
}
fn eligible(rig: &RigView, version: &str) -> Result<bool, String> {
    if !rig.online
        || rig.last_seen == 0
        || now().saturating_sub(rig.last_seen) > ONLINE_SECONDS
        || rig.snapshot.get("health").and_then(Value::as_str) != Some("online")
    {
        return Err("Rig is offline; no update was started".into());
    }
    let update = rig
        .snapshot
        .get("updates")
        .ok_or("Upgrade this rig locally once to enable Fleet updates")?;
    if update.get("protocol").and_then(Value::as_u64) != Some(1) {
        return Err("Upgrade this rig locally once to enable Fleet updates".into());
    }
    if target_reached(rig, version) {
        return Ok(true);
    }
    if update.get("enabled").and_then(Value::as_bool) != Some(true)
        || update.get("can_install").and_then(Value::as_bool) != Some(true)
    {
        return Err(
            "Remote updates require local opt-in and any required system authorization".into(),
        );
    }
    if update.get("available_version").and_then(Value::as_str) != Some(version) {
        return Err("The rig's checked update offer changed; check again before confirming".into());
    }
    Ok(false)
}

impl FleetManager {
    pub async fn create_rollout(
        &self,
        request: UpdateRolloutRequest,
    ) -> Result<UpdateRollout, String> {
        if let Some(member) = self.data()?.member {
            let response = transport::pinned_client(&member.certificate_pem)?
                .post(format!("{}/fleet/v1/update-rollouts", member.address))
                .bearer_auth(member.controller_token)
                .json(&request)
                .send()
                .await
                .map_err(|_| "Coordinator is unreachable; no update rollout was confirmed")?;
            return transport::response(response).await;
        }
        self.create_local_rollout(request).await
    }
    pub(crate) async fn create_local_rollout(
        &self,
        request: UpdateRolloutRequest,
    ) -> Result<UpdateRollout, String> {
        stable(&request.expected_version)?;
        if request.rig_ids.is_empty() || request.rig_ids.len() > 100 {
            return Err("Select between 1 and 100 rigs".into());
        }
        let local_id = self.data()?.rig_id;
        let mut ids = request.rig_ids;
        let count = ids.len();
        ids.sort();
        ids.dedup();
        if ids.len() != count {
            return Err("Duplicate update target IDs".into());
        }
        let view = self.hub_view().await?;
        let mut items = Vec::new();
        // The coordinator always goes last, independent of the caller's PC.
        ids.sort_by_key(|id| (id == &local_id, id.clone()));
        for id in ids {
            let rig = view
                .rigs
                .iter()
                .find(|r| r.rig_id == id)
                .ok_or("An update target is not enrolled")?;
            let done = eligible(rig, &request.expected_version)?;
            items.push(UpdateRolloutItem {
                rig_id: id,
                state: if done { "updated" } else { "queued" }.into(),
                error: None,
            });
        }
        let rollout = UpdateRollout {
            id: uuid::Uuid::new_v4().to_string(),
            expected_version: request.expected_version,
            state: if items.iter().all(|i| i.state == "updated") {
                "completed"
            } else {
                "running"
            }
            .into(),
            created_at: now(),
            updated_at: now(),
            rigs: items,
            error: None,
        };
        self.change(|data| {
            let hub = data
                .hub
                .as_mut()
                .filter(|h| h.enabled)
                .ok_or("Create or upgrade the Fleet coordinator first")?;
            if hub.rollouts.values().any(|p| active_plan(&p.view)) {
                return Err("A Fleet update rollout is already active".into());
            }
            if hub.rollouts.len() >= 20 {
                let oldest = hub
                    .rollouts
                    .values()
                    .min_by_key(|p| p.view.created_at)
                    .map(|p| p.view.id.clone());
                if let Some(id) = oldest {
                    hub.rollouts.remove(&id);
                }
            }
            hub.rollouts.insert(
                rollout.id.clone(),
                StoredRollout {
                    view: rollout.clone(),
                    command_id: None,
                    deadline_at: 0,
                },
            );
            Ok(())
        })?;
        Ok(rollout)
    }
    pub async fn cancel_rollout(&self, id: &str) -> Result<UpdateRollout, String> {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid rollout ID")?;
        if let Some(member) = self.data()?.member {
            let response = transport::pinned_client(&member.certificate_pem)?
                .post(format!(
                    "{}/fleet/v1/update-rollouts/{id}/cancel",
                    member.address
                ))
                .bearer_auth(member.controller_token)
                .send()
                .await
                .map_err(|_| "Coordinator is unreachable; cancellation was not confirmed")?;
            return transport::response(response).await;
        }
        self.cancel_local_rollout(id)
    }
    pub(crate) fn cancel_local_rollout(&self, id: &str) -> Result<UpdateRollout, String> {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid rollout ID")?;
        self.change(|data| {
            let plan = data
                .hub
                .as_mut()
                .and_then(|h| h.rollouts.get_mut(id))
                .ok_or("Rollout not found")?;
            if matches!(
                plan.view.state.as_str(),
                "completed" | "failed" | "cancelled"
            ) {
                return Ok(plan.view.clone());
            }
            for item in &mut plan.view.rigs {
                if item.state == "queued" {
                    item.state = "cancelled".into();
                }
            }
            plan.view.state = "cancelled".into();
            plan.view.updated_at = now();
            Ok(plan.view.clone())
        })
    }
    fn rollout_result(&self, id: &str, rig_id: &str, error: Option<String>) -> Result<(), String> {
        self.change(|data| {
            let plan = data
                .hub
                .as_mut()
                .and_then(|h| h.rollouts.get_mut(id))
                .ok_or("Rollout not found")?;
            let cancelled = plan.view.state == "cancelled";
            let item = plan
                .view
                .rigs
                .iter_mut()
                .find(|r| r.rig_id == rig_id)
                .ok_or("Rollout rig not found")?;
            item.state = if error.is_some() { "failed" } else { "updated" }.into();
            item.error = error.clone();
            plan.command_id = None;
            plan.deadline_at = 0;
            plan.view.updated_at = now();
            if !cancelled {
                if let Some(error) = error {
                    plan.view.state = "failed".into();
                    plan.view.error = Some(error);
                    for item in &mut plan.view.rigs {
                        if item.state == "queued" {
                            item.state = "cancelled".into();
                        }
                    }
                } else {
                    plan.view.state = if plan.view.rigs.iter().any(|r| r.state == "queued") {
                        "running"
                    } else {
                        "completed"
                    }
                    .into();
                }
            }
            Ok(())
        })
    }
    pub(crate) async fn tick_rollouts(&self) -> Result<(), String> {
        let data = self.data()?;
        let Some(hub) = data.hub.filter(|h| h.enabled) else {
            return Ok(());
        };
        let Some(plan) = hub
            .rollouts
            .values()
            .find(|p| active_plan(&p.view))
            .cloned()
        else {
            return Ok(());
        };
        let view = self.hub_view().await?;
        if let Some(item) = plan.view.rigs.iter().find(|r| active_item(&r.state)) {
            let rig = view.rigs.iter().find(|r| r.rig_id == item.rig_id);
            if rig.is_none() {
                return self.rollout_result(&plan.view.id, &item.rig_id, Some("Rig access was removed; an installation already committed may still finish locally".into()));
            }
            let receipt = plan.command_id.as_ref().and_then(|id| hub.commands.get(id));
            let mut error = receipt
                .filter(|c| matches!(c.receipt.state.as_str(), "failed" | "expired"))
                .map(|c| {
                    c.receipt
                        .error
                        .clone()
                        .unwrap_or_else(|| "The rig rejected its update request".into())
                });
            // A failed job for the same release may precede this retry. Bind
            // failure to the job accepted by this exact command receipt.
            let accepted_job = receipt
                .filter(|c| c.receipt.state == "succeeded")
                .and_then(|c| c.receipt.result.as_ref())
                .and_then(|r| r.pointer("/job/id"))
                .and_then(Value::as_str);
            let reported_job = rig.and_then(|r| r.snapshot.pointer("/updates/job"));
            let completed = reported_job.is_some_and(|job| {
                job.get("expected_version").and_then(Value::as_str)
                    == Some(&plan.view.expected_version)
                    && job.get("stage").and_then(Value::as_str) == Some("completed")
                    && accepted_job
                        .is_none_or(|id| job.get("id").and_then(Value::as_str) == Some(id))
            });
            if completed && rig.is_some_and(|r| target_reached(r, &plan.view.expected_version)) {
                return self.rollout_result(&plan.view.id, &item.rig_id, None);
            }
            if let Some(job) = reported_job {
                if job.get("expected_version").and_then(Value::as_str)
                    == Some(&plan.view.expected_version)
                    && accepted_job.is_some()
                    && job.get("id").and_then(Value::as_str) == accepted_job
                    && matches!(
                        job.get("stage").and_then(Value::as_str),
                        Some("failed" | "cancelled")
                    )
                {
                    error = Some(
                        job.get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("The rig's update failed")
                            .chars()
                            .take(1024)
                            .collect(),
                    );
                }
            }
            if plan.deadline_at > 0 && now() >= plan.deadline_at {
                error = Some("No target-version confirmation within 45 minutes. The installation may still be running; inspect the rig before retrying.".into());
            }
            if let Some(error) = error {
                return self.rollout_result(&plan.view.id, &item.rig_id, Some(error));
            }
            if item.rig_id == data.rig_id {
                if let Some(stored) = receipt.filter(|c| c.receipt.state == "queued") {
                    let result = self.execute_once(&stored.command).await?;
                    self.record_hub_result(&result)?;
                }
            }
            let reconnect = rig.is_none_or(|r| !r.online)
                || rig
                    .and_then(|r| r.snapshot.pointer("/updates/job/stage"))
                    .and_then(Value::as_str)
                    .is_some_and(|s| matches!(s, "installing" | "restarting" | "completed"));
            if reconnect && item.state != "waiting_for_reconnect" {
                self.change(|data| {
                    if let Some(stored) = data
                        .hub
                        .as_mut()
                        .and_then(|h| h.rollouts.get_mut(&plan.view.id))
                    {
                        if let Some(item) = stored
                            .view
                            .rigs
                            .iter_mut()
                            .find(|r| r.rig_id == item.rig_id)
                        {
                            item.state = "waiting_for_reconnect".into();
                        }
                        if stored.view.state != "cancelled" {
                            stored.view.state = "waiting_for_reconnect".into();
                        }
                        stored.view.updated_at = now();
                    }
                    Ok(())
                })?;
            }
            return Ok(());
        }
        if plan.view.state == "cancelled" {
            return Ok(());
        }
        let Some(next) = plan.view.rigs.iter().find(|r| r.state == "queued") else {
            return Ok(());
        };
        let Some(rig) = view.rigs.iter().find(|r| r.rig_id == next.rig_id) else {
            return self.rollout_result(
                &plan.view.id,
                &next.rig_id,
                Some("Rig access was revoked".into()),
            );
        };
        match eligible(rig, &plan.view.expected_version) {
            Ok(true) => self.rollout_result(&plan.view.id, &next.rig_id, None),
            Err(error) => self.rollout_result(&plan.view.id, &next.rig_id, Some(error)),
            Ok(false) => {
                // Persist command and rollout intent in one transaction. A crash
                // between these writes cannot manufacture a second installation.
                self.change(|data| {
                    let hub = data.hub.as_mut().ok_or("Coordinator is disabled")?;
                    prune_commands(hub);
                    if hub.commands.len() >= 640 {
                        return Err(
                            "Fleet command history is full; the rollout is waiting for space"
                                .into(),
                        );
                    }
                    let stored = hub
                        .rollouts
                        .get_mut(&plan.view.id)
                        .ok_or("Rollout not found")?;
                    if stored.command_id.is_some() || stored.view.state == "cancelled" {
                        return Ok(());
                    }
                    let item = stored
                        .view
                        .rigs
                        .iter_mut()
                        .find(|r| r.rig_id == next.rig_id)
                        .ok_or("Rollout rig not found")?;
                    if item.state != "queued" {
                        return Ok(());
                    }
                    let command = FleetCommand {
                        id: uuid::Uuid::new_v4().to_string(),
                        rig_id: next.rig_id.clone(),
                        created_at: now(),
                        expires_at: now() + DEFAULT_COMMAND_SECONDS,
                        operation: FleetOperation::UpdateInstall {
                            expected_version: plan.view.expected_version.clone(),
                        },
                    };
                    stored.command_id = Some(command.id.clone());
                    stored.deadline_at = now() + CONFIRM_SECONDS;
                    item.state = "installing".into();
                    stored.view.updated_at = now();
                    hub.commands.insert(
                        command.id.clone(),
                        StoredCommand {
                            receipt: CommandReceipt::queued(&command),
                            command,
                        },
                    );
                    Ok(())
                })
            }
        }
    }
}
