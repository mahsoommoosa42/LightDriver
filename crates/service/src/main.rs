mod controller;
use lightdriver_core::{LogLevel, LogStore, Mode, PROTOCOL_VERSION, Snapshot};
use lightdriver_devices::{adapter, poll_snapshot};
use lightdriver_ipc::{Request, RequestAction, bind, read_frame, write_frame};
use lightdriver_monitoring::Collector;
use std::{io, sync::Arc, time::Duration};
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mode = if args.iter().any(|a| a == "--demo") {
        Mode::Demo
    } else {
        Mode::Live
    };
    if args.iter().any(|a| a == "--help") {
        println!(
            "LightDriver local service\n\n  --demo      Simulated data; never opens hardware\n  --once      Read one snapshot, print JSON, exit\n  --snapshot  Read the running service, print JSON, exit\n  --logs      Read the running service log and configuration\n  --log-level <error|warning|info|debug|verbose>  Set verbosity\n\nDefault: per-user local IPC service; explicit opt-in control. Ctrl+C exits."
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--cpu-sensor-handoff") {
        println!(
            "{}",
            lightdriver_ipc::stop_unprivileged_live_service(mode).await?
        );
        return Ok(());
    }
    if let Some(path) = args.windows(2).find(|a| a[0] == "--command").map(|a| &a[1]) {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(8193)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 8192 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Command file exceeds 8 KiB.",
            ));
        }
        let request: lightdriver_core::ControlRequest = serde_json::from_slice(&bytes)?;
        let receipt = lightdriver_ipc::control(mode, request).await?;
        println!("{}", serde_json::to_string_pretty(&receipt)?);
        return Ok(());
    }
    if args.iter().any(|a| a == "--logs") {
        let level = args
            .windows(2)
            .find(|a| a[0] == "--log-level")
            .map(|a| {
                LogLevel::parse(&a[1]).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "Invalid log level.")
                })
            })
            .transpose()?;
        let batch = lightdriver_ipc::logs(mode, 0, level).await?;
        println!("{}", serde_json::to_string_pretty(&batch)?);
        return Ok(());
    }
    if args.iter().any(|a| a == "--snapshot") {
        let snapshot = lightdriver_ipc::snapshot(mode).await?;
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
        return Ok(());
    }
    if args.iter().any(|a| a == "--once") {
        let snapshot = tokio::task::spawn_blocking(move || {
            let mut snapshot = poll_snapshot(adapter(mode).as_mut(), mode);
            snapshot.diagnostics = Collector::new(mode).poll();
            snapshot
        })
        .await
        .map_err(io::Error::other)?;
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
        return Ok(());
    }

    // Claim the IPC name before opening any hardware. One owner per mode/user.
    let listener = bind(mode)?;
    let logger = LogStore::default();
    if let Some(value) = args
        .windows(2)
        .find(|a| a[0] == "--log-level")
        .map(|a| &a[1])
    {
        let level = LogLevel::parse(value)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid log level."))?;
        logger.read(mode, 0, Some(level));
    }
    let os = lightdriver_monitoring::system_description(mode);
    logger.event(
        LogLevel::Info,
        "service",
        format!(
            "LightDriver {} · protocol {} · {:?} · {os}",
            env!("CARGO_PKG_VERSION"),
            PROTOCOL_VERSION,
            mode
        ),
    );
    let diagnostic_logger = logger.clone();
    let state = Arc::new(RwLock::new(Snapshot::empty(mode)));
    let diagnostics = Arc::new(RwLock::new(lightdriver_core::Diagnostics::default()));
    let diagnostic_state = diagnostics.clone();
    let stop_monitor = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_worker = stop_monitor.clone();
    // Vendor APIs stay on a dedicated worker. No vendor handle crosses threads, and
    // a slow GPU collector cannot block IPC snapshots or Kraken polling.
    let _diagnostic_thread = std::thread::Builder::new()
        .name("system-diagnostics".into())
        .spawn(move || {
            let mut collector = Collector::new(mode);
            let mut provider_states = Vec::new();
            let mut config_seen = false;
            let mut sensor_states = Vec::new();
            while !stop_worker.load(std::sync::atomic::Ordering::Relaxed) {
                let data = collector.poll();
                if !config_seen {
                    let mut snapshot = Snapshot::empty(mode);
                    snapshot.diagnostics = data.clone();
                    for line in lightdriver_core::log_system_summary(
                        &snapshot,
                        "See service startup OS description",
                    )
                    .into_iter()
                    .filter(|line| {
                        !line.starts_with("Controller:")
                            && !line.starts_with("Adapter:")
                            && !line.starts_with("OS:")
                    }) {
                        diagnostic_logger.event(LogLevel::Info, "configuration", line);
                    }
                    config_seen = true;
                }
                let providers: Vec<_> = data
                    .providers
                    .iter()
                    .map(|p| (p.name.clone(), p.detail.clone()))
                    .collect();
                for (name, detail) in &providers {
                    if !provider_states.contains(&(name.clone(), detail.clone())) {
                        diagnostic_logger.event(
                            LogLevel::Info,
                            "sensors",
                            format!("{name} · {detail}"),
                        );
                    }
                }
                provider_states = providers;
                let sensors: Vec<_> = data
                    .sensors
                    .iter()
                    .map(|s| (s.id.clone(), s.status, s.detail.clone()))
                    .collect();
                for sensor in &data.sensors {
                    if !sensor_states.contains(&(
                        sensor.id.clone(),
                        sensor.status,
                        sensor.detail.clone(),
                    )) {
                        diagnostic_logger.event(
                            if sensor.status == lightdriver_core::SensorStatus::Error {
                                LogLevel::Error
                            } else if sensor.sample.is_none() {
                                LogLevel::Warning
                            } else {
                                LogLevel::Debug
                            },
                            "sensors",
                            format!("{} · {:?} · {}", sensor.label, sensor.status, sensor.detail),
                        );
                    }
                    if diagnostic_logger.enabled(LogLevel::Verbose)
                        && let Some(sample) = &sensor.sample
                    {
                        diagnostic_logger.event(
                            LogLevel::Verbose,
                            "sensors.decoded",
                            format!(
                                "{} · {} · {} {} · sample {} · {}",
                                sensor.group,
                                sensor.label,
                                sample.value,
                                sensor.unit.label(),
                                sample.at_ms,
                                sensor.source
                            ),
                        );
                    }
                }
                sensor_states = sensors;
                *diagnostic_state.blocking_write() = data;
                std::thread::sleep(Duration::from_secs(1));
            }
        })?;
    let command_sender =
        controller::start(mode, state.clone(), stop_monitor.clone(), logger.clone())?;
    println!(
        "LightDriver {:?} service ready. Local IPC; device control requires explicit enable.",
        mode
    );

    // Bound client tasks; a stalled client cannot stop hardware polling.
    let permits = Arc::new(tokio::sync::Semaphore::new(16));
    let handoff = Arc::new(tokio::sync::Notify::new());
    let handoff_allowed = lightdriver_ipc::permits_cpu_sensor_handoff(
        mode,
        lightdriver_monitoring::is_administrator().unwrap_or(true),
    );
    loop {
        let stream = tokio::select! {
            accepted = lightdriver_ipc::accept(&listener) => accepted?,
            _ = tokio::signal::ctrl_c() => break,
            _ = handoff.notified() => break,
        };
        let Ok(permit) = permits.clone().try_acquire_owned() else {
            continue;
        };
        let state = state.clone();
        let diagnostics = diagnostics.clone();
        let handoff = handoff.clone();
        let command_sender = command_sender.clone();
        let logger = logger.clone();
        let os = os.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let (mut reader, mut writer) = lightdriver_ipc::split(stream);
            if let Ok(request) = read_frame::<Request>(&mut reader).await {
                if request.protocol_version != PROTOCOL_VERSION {
                    return;
                }
                match request.action {
                    RequestAction::Logs { after, level } => {
                        let old_level = logger.level();
                        let mut batch = logger.read(mode, after, level);
                        if old_level != batch.level {
                            logger.event(
                                LogLevel::Info,
                                "logging",
                                format!(
                                    "Verbosity: {} · raw capture {}",
                                    batch.level,
                                    batch.level == LogLevel::Verbose
                                ),
                            );
                        }
                        let mut snapshot = state.read().await.clone();
                        snapshot.diagnostics = diagnostics.read().await.clone();
                        batch.system_summary = lightdriver_core::log_system_summary(&snapshot, &os);
                        let _ = write_frame(&mut writer, &batch).await;
                    }
                    RequestAction::Snapshot => {
                        let mut snapshot = state.read().await.clone();
                        snapshot.diagnostics = diagnostics.read().await.clone();
                        let _ = write_frame(&mut writer, &snapshot).await;
                    }
                    RequestAction::StopUnprivilegedLiveService => {
                        let replied = write_frame(&mut writer, &handoff_allowed).await;
                        if replied.is_ok() && handoff_allowed {
                            handoff.notify_one();
                        }
                    }
                    RequestAction::Control(request) => {
                        let (reply, answer) = tokio::sync::oneshot::channel();
                        let rejected = lightdriver_core::CommandReceipt::rejected(
                            &request,
                            "Control queue is busy. Command was not accepted.",
                        );
                        if command_sender
                            .try_send(controller::Envelope { request, reply })
                            .is_err()
                        {
                            logger.event(LogLevel::Warning, "control", &rejected.detail);
                            let _ = write_frame(&mut writer, &rejected).await;
                            return;
                        }
                        if let Ok(Ok(receipt)) =
                            tokio::time::timeout(Duration::from_secs(7), answer).await
                        {
                            let _ = write_frame(&mut writer, &receipt).await;
                        }
                    }
                }
            }
        });
    }
    logger.event(LogLevel::Info, "service", "Local service stopping.");
    stop_monitor.store(true, std::sync::atomic::Ordering::Relaxed);

    Ok(())
}
