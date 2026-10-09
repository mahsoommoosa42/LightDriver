use lightdriver_core::{Mode, PROTOCOL_VERSION, Snapshot};
use lightdriver_devices::{adapter, poll_snapshot};
use lightdriver_ipc::{Request, bind, read_frame, write_frame};
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
            "LightDriver local service\n\n  --demo      Simulated data; never opens hardware\n  --once      Read one snapshot, print JSON, exit\n  --snapshot  Read the running service, print JSON, exit\n\nDefault: per-user local IPC service with read-only telemetry. Ctrl+C exits."
        );
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
            while !stop_worker.load(std::sync::atomic::Ordering::Relaxed) {
                *diagnostic_state.blocking_write() = collector.poll();
                std::thread::sleep(Duration::from_secs(1));
            }
        })?;
    let poll_state = state.clone();
    let poller = tokio::spawn(async move {
        let mut current_adapter = adapter(mode);
        loop {
            let (returned_adapter, snapshot) = match tokio::task::spawn_blocking(move || {
                let snapshot = poll_snapshot(current_adapter.as_mut(), mode);
                (current_adapter, snapshot)
            })
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("Telemetry worker stopped: {error}");
                    break;
                }
            };
            current_adapter = returned_adapter;
            *poll_state.write().await = snapshot;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    println!(
        "LightDriver {:?} service ready. Local IPC only; hardware writes disabled.",
        mode
    );

    // Bound client tasks; a stalled client cannot stop hardware polling.
    let permits = Arc::new(tokio::sync::Semaphore::new(16));
    loop {
        let stream = tokio::select! {
            accepted = lightdriver_ipc::accept(&listener) => accepted?,
            _ = tokio::signal::ctrl_c() => break,
        };
        let Ok(permit) = permits.clone().try_acquire_owned() else {
            continue;
        };
        let state = state.clone();
        let diagnostics = diagnostics.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let (mut reader, mut writer) = lightdriver_ipc::split(stream);
            match read_frame::<Request>(&mut reader).await {
                Ok(request) if request.protocol_version == PROTOCOL_VERSION => {
                    let mut snapshot = state.read().await.clone();
                    snapshot.diagnostics = diagnostics.read().await.clone();
                    let _ = write_frame(&mut writer, &snapshot).await;
                }
                _ => {}
            }
        });
    }
    stop_monitor.store(true, std::sync::atomic::Ordering::Relaxed);
    poller.abort();
    Ok(())
}
