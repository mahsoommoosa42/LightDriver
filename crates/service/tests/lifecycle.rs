//! Exercise real local IPC and ownership without opening hardware.
use lightdriver_core::{Mode, Snapshot};
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
