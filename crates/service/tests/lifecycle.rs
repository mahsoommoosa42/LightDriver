//! Exercise real local IPC and ownership without opening hardware.
use lightdriver_core::{
    CommandReceipt, CommandState, ControlAction, ControlRequest, Draft, Mode, Snapshot, now_ms,
};
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct OwnedService(Child);
impl Drop for OwnedService {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command(scope: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lightdriver-service"));
    // Isolate test endpoints without changing the test runner's environment.
    command
        .env("USERDOMAIN", scope)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

#[test]
fn demo_service_owns_endpoint_survives_clients_and_never_falls_back_to_live() {
    let scope = format!("lightdriver-lifecycle-test-{}", std::process::id());
    let mut service = OwnedService(
        command(&scope)
            .arg("--demo")
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let output = command(&scope)
            .args(["--demo", "--snapshot"])
            .output()
            .unwrap();
        if output.status.success() {
            let snapshot: Snapshot = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(snapshot.mode, Mode::Demo);
            if snapshot.devices[0].telemetry.is_some() && !snapshot.diagnostics.sensors.is_empty() {
                assert!(
                    snapshot
                        .diagnostics
                        .sensors
                        .iter()
                        .filter(|s| s.sample.is_some())
                        .all(|s| s.source == "Simulated data")
                );
                assert!(serde_json::to_vec(&snapshot).unwrap().len() < lightdriver_ipc::MAX_FRAME);
                break;
            }
        }
        assert!(
            service.0.try_wait().unwrap().is_none(),
            "Service exited during startup"
        );
        assert!(
            Instant::now() < deadline,
            "Service did not publish demo telemetry"
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    let mut duplicate = OwnedService(
        command(&scope)
            .arg("--demo")
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = duplicate.0.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Duplicate service acquired the same endpoint"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !command(&scope)
            .arg("--snapshot")
            .output()
            .unwrap()
            .status
            .success(),
        "Live client must not receive demo data"
    );
    let output = command(&scope)
        .args(["--demo", "--snapshot"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Service must survive earlier client exits"
    );
    // Logging uses its own bounded response and follows the selected verbosity.
    let read_logs = |level: &str| -> lightdriver_core::LogBatch {
        let output = command(&scope)
            .args(["--demo", "--logs", "--log-level", level])
            .output()
            .unwrap();
        assert!(output.status.success());
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let info = read_logs("info");
    assert_eq!(info.mode, Mode::Demo);
    assert!(!info.system_summary.is_empty());
    assert!(
        info.system_summary
            .iter()
            .any(|line| line.contains("physical cores"))
    );
    assert!(info.entries.iter().all(|entry| !entry.raw));
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let batch = read_logs("verbose");
        assert!(
            batch
                .entries
                .iter()
                .all(|entry| entry.visible_at(batch.level))
        );
        if batch.entries.iter().any(|entry| entry.raw) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Demo raw stream did not appear in Verbose."
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let info = read_logs("info");
    assert!(
        info.entries
            .iter()
            .all(|entry| !entry.raw && entry.level <= lightdriver_core::LogLevel::Info)
    );
    assert!(serde_json::to_vec(&info).unwrap().len() < lightdriver_ipc::MAX_FRAME);
    // Real typed commands traverse the named pipe to the serialized demo actor.
    let path = std::env::temp_dir().join(format!(
        "lightdriver-command-test-{}.json",
        std::process::id()
    ));
    let submit = |r: &ControlRequest| -> CommandReceipt {
        std::fs::write(&path, serde_json::to_vec(r).unwrap()).unwrap();
        let output = command(&scope)
            .arg("--demo")
            .arg("--command")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Command client failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let request = |id: &str, action: ControlAction| ControlRequest {
        request_id: id.into(),
        device_id: "demo-kraken-x3".into(),
        mode: Mode::Demo,
        expires_at_ms: now_ms() + 10_000,
        action,
    };
    assert_eq!(
        submit(&request("locked", ControlAction::PumpFull)).state,
        CommandState::Rejected
    );
    let enable = request("enable", ControlAction::Enable);
    assert_eq!(submit(&enable).state, CommandState::Simulated);
    assert_eq!(submit(&enable).state, CommandState::Simulated);
    let mut conflict = enable.clone();
    conflict.action = ControlAction::Release;
    assert_eq!(submit(&conflict).state, CommandState::Rejected);
    assert_eq!(
        submit(&request(
            "curve",
            ControlAction::PumpCurve(Draft::default().curve.full_speed())
        ))
        .state,
        CommandState::Simulated
    );
    assert_eq!(
        submit(&request(
            "lighting",
            ControlAction::Lighting(Draft::default().lighting())
        ))
        .state,
        CommandState::Simulated
    );
    let mut board_request = request("board-enable", ControlAction::Enable);
    board_request.device_id = "motherboard".into();
    assert_eq!(submit(&board_request).state, CommandState::Simulated);
    board_request.request_id = "board-light".into();
    board_request.action = ControlAction::HeaderLighting {
        channel_id: "argb-2".into(),
        settings: Draft::default().lighting(),
    };
    assert_eq!(submit(&board_request).state, CommandState::Simulated);
    board_request.request_id = "board-unknown".into();
    board_request.action = ControlAction::HeaderLighting {
        channel_id: "argb-3".into(),
        settings: Draft::default().lighting(),
    };
    assert_eq!(submit(&board_request).state, CommandState::Rejected);
    let mut wrong_target = request("target", ControlAction::PumpFull);
    wrong_target.device_id = "motherboard".into();
    assert_eq!(submit(&wrong_target).state, CommandState::Rejected);
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let output = command(&scope)
            .args(["--demo", "--snapshot"])
            .output()
            .unwrap();
        let snapshot: Snapshot = serde_json::from_slice(&output.stdout).unwrap();
        if snapshot.devices[0].telemetry.as_ref().unwrap().pump_duty == 100 {
            assert!(snapshot.devices[0].control.enabled);
            assert!(
                snapshot
                    .command_receipts
                    .iter()
                    .any(|r| r.action == "Pump curve" && r.state == CommandState::Simulated)
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Simulated curve did not reach the snapshot."
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        submit(&request("release", ControlAction::Release)).state,
        CommandState::Simulated
    );
    assert_eq!(
        submit(&request("released", ControlAction::PumpFull)).state,
        CommandState::Rejected
    );
    std::fs::remove_file(path).unwrap();

    // The actual demo server rejects the handoff command and remains available.
    let handoff = command(&scope)
        .args(["--demo", "--cpu-sensor-handoff"])
        .output()
        .unwrap();
    assert!(handoff.status.success());
    assert_eq!(String::from_utf8_lossy(&handoff.stdout).trim(), "false");
    assert!(service.0.try_wait().unwrap().is_none());
}

#[test]
fn diagnostic_inventory_fits_the_ipc_frame_budget() {
    let mut collector = lightdriver_monitoring::Collector::new(Mode::Demo);
    let mut data = collector.poll();
    for _ in 0..100 {
        data = collector.poll();
    }
    let prototype = data
        .sensors
        .into_iter()
        .find(|s| s.sample.is_some())
        .unwrap();
    let mut snapshot = Snapshot::empty(Mode::Demo);
    snapshot.diagnostics.cpu_topology = data.cpu_topology;
    for i in 0..lightdriver_core::SENSOR_LIMIT {
        let mut sensor = prototype.clone();
        sensor.id = format!("gpu.{i}.temperature");
        sensor.group = "G".repeat(256);
        sensor.label = "S".repeat(64);
        sensor.source = "P".repeat(64);
        snapshot.diagnostics.sensors.push(sensor);
    }
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    assert!(
        bytes.len() < lightdriver_ipc::MAX_FRAME,
        "Diagnostics exceeded IPC budget: {}",
        bytes.len()
    );
}
