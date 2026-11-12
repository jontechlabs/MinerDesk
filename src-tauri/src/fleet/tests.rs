use super::*;
use std::sync::atomic::AtomicUsize;

#[derive(Default)]
struct FixtureCore {
    calls: AtomicUsize,
}
impl FleetCore for FixtureCore {
    fn snapshot(&self) -> Result<Value, String> {
        Ok(
            json!({"health":"online","name":"fixture","os":"linux","arch":"x86_64","version":"fixture","local_time":"2026-11-12T12:00:00Z","timezone":"+00:00","utc_offset_seconds":0,"status":[],"revision":"1","gpu_inventory":[],"gpu_inventory_available":false}),
        )
    }
    fn config(&self) -> Result<Value, String> {
        Ok(
            json!({"revision":"1","profiles":[],"schedules":[],"local_time":"2026-11-12T12:00:00Z","timezone":"+00:00","utc_offset_seconds":0,"editable_profile_fields":[]}),
        )
    }
    fn execute(&self, _: &FleetOperation, expires: u64) -> Result<Value, String> {
        if now() >= expires {
            return Err("Expired fixture command".into());
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"ok":true}))
    }
}
fn manager(directory: &tempfile::TempDir, core: Arc<FixtureCore>) -> Arc<FleetManager> {
    FleetManager::new(directory.path().join("config.json"), core).unwrap()
}
async fn coordinator(manager: &Arc<FleetManager>) -> FleetView {
    manager
        .create(CreateHub {
            name: "Fixture coordinator".into(),
            advertise_url: "https://localhost:0".into(),
            listen: "127.0.0.1:0".into(),
        })
        .await
        .unwrap()
}
fn operation_request(rig: String) -> CommandRequest {
    CommandRequest {
        rig_ids: vec![rig],
        operation: FleetOperation::Start {
            profile_id: "fixture".into(),
        },
        expires_in_seconds: Some(30),
    }
}

#[tokio::test]
async fn join_with_only_address_and_key_uses_the_machine_name() {
    let hub_dir = tempfile::tempdir().unwrap();
    let rig_dir = tempfile::tempdir().unwrap();
    let hub = manager(&hub_dir, Arc::new(FixtureCore::default()));
    let rig = manager(&rig_dir, Arc::new(FixtureCore::default()));
    coordinator(&hub).await;
    let invitation = hub
        .invite(InviteRequest {
            expires_in_seconds: 300,
        })
        .await
        .unwrap();
    let request: JoinRequest = serde_json::from_value(json!({
        "address": invitation.address,
        "key": invitation.key,
    }))
    .unwrap();
    assert!(request.name.is_empty());
    rig.join(request).await.unwrap();
    let setup = rig.local_status();
    assert_eq!(setup.name, local_name());
    assert_eq!(
        hub.data().unwrap().hub.unwrap().rigs[&setup.rig_id].name,
        setup.name
    );
    let reopened = manager(&rig_dir, Arc::new(FixtureCore::default()));
    assert_eq!(reopened.local_status().name, setup.name);
    rig.leave().await.unwrap();
    hub.disable().await.unwrap();
}

#[tokio::test]
async fn two_instances_pair_over_verified_tls_and_command_receipts_survive_retry() {
    let hub_dir = tempfile::tempdir().unwrap();
    let rig_dir = tempfile::tempdir().unwrap();
    let hub = manager(&hub_dir, Arc::new(FixtureCore::default()));
    let core = Arc::new(FixtureCore::default());
    let rig = manager(&rig_dir, core.clone());
    coordinator(&hub).await;
    let invitation = hub
        .invite(InviteRequest {
            expires_in_seconds: 300,
        })
        .await
        .unwrap();
    rig.join(JoinRequest {
        address: invitation.address.clone(),
        key: invitation.key.clone(),
        name: "Fixture rig".into(),
    })
    .await
    .unwrap();
    assert_eq!(rig.local_status().name, "Fixture rig");
    let rig_id = rig.local_status().rig_id;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if hub
            .view()
            .await
            .unwrap()
            .rigs
            .iter()
            .any(|r| r.rig_id == rig_id && r.online)
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Agent never became online"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let queued = rig
        .command(operation_request(rig_id.clone()))
        .await
        .unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].state, "queued");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if hub.receipt(&queued[0].id).await.unwrap().state == "succeeded" {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Command did not finish"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(core.calls.load(Ordering::SeqCst), 1);
    let stored = hub.data().unwrap().hub.unwrap().commands[&queued[0].id]
        .command
        .clone();
    assert_eq!(rig.execute_once(&stored).await.unwrap().state, "succeeded");
    assert_eq!(core.calls.load(Ordering::SeqCst), 1);
    // A new process instance must consult the persisted journal, too.
    let reopened = manager(&rig_dir, core.clone());
    assert_eq!(
        reopened.execute_once(&stored).await.unwrap().state,
        "succeeded"
    );
    assert_eq!(core.calls.load(Ordering::SeqCst), 1);
    let second_dir = tempfile::tempdir().unwrap();
    let second = manager(&second_dir, Arc::new(FixtureCore::default()));
    assert!(
        second
            .join(JoinRequest {
                address: invitation.address,
                key: invitation.key,
                name: "Second rig".into()
            })
            .await
            .is_err(),
        "Used invitation was accepted"
    );
    let private = rig.data().unwrap().member.unwrap();
    assert!(hub.authorize_controller(&private.agent_token).is_err());
    assert!(hub
        .receive_poll(
            &private.controller_token,
            PollRequest {
                snapshot: core.snapshot().unwrap(),
                config: core.config().unwrap(),
                results: vec![]
            }
        )
        .is_err());
    let public = serde_json::to_string(&hub.view().await.unwrap()).unwrap();
    assert!(!public.contains(&private.agent_token));
    assert!(!public.contains(&private.controller_token));
    assert!(!public.contains("PRIVATE KEY"));
    hub.revoke(&rig_id).await.unwrap();
    assert!(rig.command(operation_request(rig_id)).await.is_err());
    rig.leave().await.unwrap();
    hub.disable().await.unwrap();
}

#[tokio::test]
async fn wrong_certificate_fails_before_invitation_is_consumed() {
    let hub_dir = tempfile::tempdir().unwrap();
    let rig_dir = tempfile::tempdir().unwrap();
    let hub = manager(&hub_dir, Arc::new(FixtureCore::default()));
    let rig = manager(&rig_dir, Arc::new(FixtureCore::default()));
    // An external HTTPS port may forward to a different internal listener port.
    // All actual test connections stay on a random loopback port.
    let reserve = TcpListener::bind("127.0.0.1:0").unwrap();
    let bind = reserve.local_addr().unwrap();
    drop(reserve);
    hub.create(CreateHub {
        name: "Public-name fixture".into(),
        advertise_url: "https://fleet.example.invalid:443".into(),
        listen: bind.to_string(),
    })
    .await
    .unwrap();
    let invitation = hub
        .invite(InviteRequest {
            expires_in_seconds: 300,
        })
        .await
        .unwrap();
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&invitation.key)
        .unwrap();
    let mut false_key: InvitationKey = serde_json::from_slice(&decoded).unwrap();
    false_key.certificate_pem = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .unwrap()
        .cert
        .pem();
    false_key.certificate_fingerprint = transport::fingerprint(&false_key.certificate_pem).unwrap();
    let key = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&false_key).unwrap());
    let alternate = format!("https://{bind}");
    assert!(rig
        .join(JoinRequest {
            address: alternate.clone(),
            key,
            name: "Fixture rig".into()
        })
        .await
        .is_err());
    assert_eq!(hub.data().unwrap().hub.unwrap().invitations.len(), 1);
    // A different HTTPS address is allowed only when the same pinned identity
    // and that address's hostname are verified (LAN/public alias support).
    assert!(invitation.address.contains("fleet.example.invalid"));
    rig.join(JoinRequest {
        address: alternate,
        key: invitation.key,
        name: "Fixture rig".into(),
    })
    .await
    .unwrap();
    rig.leave().await.unwrap();
    hub.disable().await.unwrap();
}

#[tokio::test]
async fn heartbeats_do_not_rewrite_credentials_and_invalid_dtos_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let core = Arc::new(FixtureCore::default());
    let hub = manager(&dir, core.clone());
    coordinator(&hub).await;
    let invitation = hub
        .invite(InviteRequest {
            expires_in_seconds: 300,
        })
        .await
        .unwrap();
    let key: InvitationKey = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(invitation.key)
            .unwrap(),
    )
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let enrolled = hub
        .enroll(EnrollRequest {
            code: key.code,
            rig_id: id.clone(),
            name: "Heartbeat fixture".into(),
        })
        .unwrap();
    let path = dir.path().join("fleet-private/state.json");
    let before = std::fs::metadata(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    for _ in 0..4 {
        hub.receive_poll(
            &enrolled.agent_token,
            PollRequest {
                snapshot: core.snapshot().unwrap(),
                config: core.config().unwrap(),
                results: vec![],
            },
        )
        .unwrap();
    }
    let after = std::fs::metadata(&path).unwrap();
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_eq!(bytes, std::fs::read(&path).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(before.ino(), after.ino());
    }
    assert!(hub
        .view()
        .await
        .unwrap()
        .rigs
        .iter()
        .any(|r| r.rig_id == id && r.online));
    let mut poisoned = core.snapshot().unwrap();
    poisoned["status"] = Value::Null;
    assert!(hub
        .receive_poll(
            &enrolled.agent_token,
            PollRequest {
                snapshot: poisoned,
                config: core.config().unwrap(),
                results: vec![]
            }
        )
        .is_err());
    let reopened = manager(&dir, core);
    assert_eq!(
        reopened.data().unwrap().hub.unwrap().rigs[&id].last_seen,
        0,
        "Restart must not revive stale online telemetry"
    );
    hub.disable().await.unwrap();
}

#[tokio::test]
async fn expired_commands_and_reused_ids_cannot_start_a_miner() {
    let dir = tempfile::tempdir().unwrap();
    let core = Arc::new(FixtureCore::default());
    let rig = manager(&dir, core.clone());
    let mut command = FleetCommand {
        id: uuid::Uuid::new_v4().to_string(),
        rig_id: rig.local_status().rig_id,
        created_at: now().saturating_sub(60),
        expires_at: now().saturating_sub(1),
        operation: FleetOperation::Start {
            profile_id: "fixture".into(),
        },
    };
    assert_eq!(rig.execute_once(&command).await.unwrap().state, "expired");
    assert_eq!(core.calls.load(Ordering::SeqCst), 0);
    command.expires_at = now() + 30;
    assert!(rig.execute_once(&command).await.is_err());
    assert_eq!(core.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn journal_pressure_does_not_evict_a_replayable_command() {
    let dir = tempfile::tempdir().unwrap();
    let core = Arc::new(FixtureCore::default());
    let rig = manager(&dir, core.clone());
    let command = FleetCommand {
        id: "00000000-target".into(),
        rig_id: rig.local_status().rig_id,
        created_at: now(),
        expires_at: now() + 60,
        operation: FleetOperation::Start {
            profile_id: "fixture".into(),
        },
    };
    rig.execute_once(&command).await.unwrap();
    rig.change(|data| {
        for i in 0..514 {
            let mut receipt = CommandReceipt::queued(&command);
            receipt.id = format!("fixture-{i}");
            receipt.state = "succeeded".into();
            data.executed.insert(
                receipt.id.clone(),
                Executed {
                    fingerprint: "fixture".into(),
                    receipt,
                    delivered: true,
                },
            );
        }
        Ok(())
    })
    .unwrap();
    let next = FleetCommand {
        id: uuid::Uuid::new_v4().to_string(),
        ..command.clone()
    };
    rig.execute_once(&next).await.unwrap();
    rig.execute_once(&command).await.unwrap();
    assert_eq!(core.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn identities_persist_outside_app_config_and_failed_oversized_saves_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let core = Arc::new(FixtureCore::default());
    let rig = manager(&dir, core.clone());
    let before = rig.local_status().rig_id;
    let path = dir.path().join("fleet-private/state.json");
    assert!(path.exists());
    assert!(!dir.path().join("config.json").exists());
    assert!(rig
        .change(|data| {
            data.name = "x".repeat(17 * 1024 * 1024);
            Ok(())
        })
        .is_err());
    assert_eq!(rig.local_status().rig_id, before);
    let reopened = manager(&dir, core);
    assert_eq!(reopened.local_status().rig_id, before);
    assert!(reopened.local_status().name.len() < 100);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[tokio::test]
async fn abandoned_expired_commands_do_not_permanently_fill_the_hub_queue() {
    let dir = tempfile::tempdir().unwrap();
    let core = Arc::new(FixtureCore::default());
    let hub = manager(&dir, core.clone());
    coordinator(&hub).await;
    hub.change(|data| {
        let record = data.hub.as_mut().unwrap();
        for index in 0..640 {
            let command = FleetCommand {
                id: format!("abandoned-{index}"),
                rig_id: "offline-fixture".into(),
                created_at: now().saturating_sub(120),
                expires_at: now().saturating_sub(1),
                operation: FleetOperation::Start {
                    profile_id: "fixture".into(),
                },
            };
            record.commands.insert(
                command.id.clone(),
                StoredCommand {
                    receipt: CommandReceipt::queued(&command),
                    command,
                },
            );
        }
        Ok(())
    })
    .unwrap();
    let receipts = hub
        .command(operation_request(hub.local_status().rig_id))
        .await
        .unwrap();
    assert_eq!(receipts[0].state, "succeeded");
    assert_eq!(core.calls.load(Ordering::SeqCst), 1);
    assert!(hub.data().unwrap().hub.unwrap().commands.len() < 640);
    hub.disable().await.unwrap();
}

#[tokio::test]
async fn leaving_revokes_both_own_credentials_and_cannot_target_a_neighbor() {
    let directories: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
    let hub = manager(&directories[0], Arc::new(FixtureCore::default()));
    let first = manager(&directories[1], Arc::new(FixtureCore::default()));
    let second = manager(&directories[2], Arc::new(FixtureCore::default()));
    coordinator(&hub).await;
    assert_eq!(hub.local_status().name, "Fixture coordinator");
    for rig in [&first, &second] {
        let invite = hub
            .invite(InviteRequest {
                expires_in_seconds: 300,
            })
            .await
            .unwrap();
        rig.join(JoinRequest {
            address: invite.address,
            key: invite.key,
            name: "Owned fixture".into(),
        })
        .await
        .unwrap();
    }
    let credential = first.data().unwrap().member.unwrap();
    let neighbor = second.data().unwrap().member.unwrap();
    let request = transport::pinned_client(&credential.certificate_pem)
        .unwrap()
        .post(format!("{}/fleet/v1/leave", credential.address))
        .bearer_auth(&credential.agent_token)
        .json(&json!({"rig_id":second.local_status().rig_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(request.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(hub.authorize_controller(&neighbor.controller_token).is_ok());
    let view = first.leave().await.unwrap();
    assert_eq!(view.setup.mode, "standalone");
    assert!(view.setup.error.is_none());
    assert!(hub
        .authorize_controller(&credential.controller_token)
        .is_err());
    assert!(hub.revoke_self(&credential.agent_token).is_err());
    assert!(hub.authorize_controller(&neighbor.controller_token).is_ok());
    assert_eq!(hub.data().unwrap().hub.unwrap().rigs.len(), 1);
    second.leave().await.unwrap();
    hub.disable().await.unwrap();
}
