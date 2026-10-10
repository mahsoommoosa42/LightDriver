//! Serialized device polling/commands. Deduplication lasts for the full command deadline.
use lightdriver_core::{
    CommandReceipt, ControlRequest, LogLevel, LogStore, Mode, Snapshot, now_ms,
};
use lightdriver_devices::{DeviceAdapter, adapter, poll_snapshot};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use tokio::sync::{RwLock, oneshot};
const CACHE_LIMIT: usize = 64;
type ReceiptCache = VecDeque<(ControlRequest, CommandReceipt)>;
pub struct Envelope {
    pub request: ControlRequest,
    pub reply: oneshot::Sender<CommandReceipt>,
}
fn dispatch(
    adapter: &mut dyn DeviceAdapter,
    mode: Mode,
    request: &ControlRequest,
    client_closed: bool,
    cache: &mut ReceiptCache,
    now: u64,
) -> CommandReceipt {
    cache.retain(|(r, _)| r.expires_at_ms >= now);
    if let Some((prior, receipt)) = cache
        .iter()
        .find(|(r, _)| r.request_id == request.request_id)
    {
        return if prior == request {
            receipt.clone()
        } else {
            CommandReceipt::rejected(
                request,
                "Command identifier was already used for another payload.",
            )
        };
    }
    if let Err(e) = request.validate_at(now) {
        return CommandReceipt::rejected(request, e);
    }
    // Never evict a valid command to make room: duplicates must not execute again.
    if cache.len() >= CACHE_LIMIT {
        return CommandReceipt::rejected(
            request,
            "Command receipt cache is full. Command was not executed.",
        );
    }
    let receipt = if client_closed {
        CommandReceipt::rejected(request, "Client disconnected before command execution.")
    } else if request.mode != mode {
        CommandReceipt::rejected(request, "Live/demo command mode mismatch.")
    } else {
        adapter.control(request)
    };
    cache.push_back((request.clone(), receipt.clone()));
    receipt
}
pub fn start(
    mode: Mode,
    state: Arc<RwLock<Snapshot>>,
    stop: Arc<AtomicBool>,
    logger: LogStore,
) -> std::io::Result<mpsc::SyncSender<Envelope>> {
    let (send, receive) = mpsc::sync_channel::<Envelope>(8);
    std::thread::Builder::new()
        .name("device-control".into())
        .spawn(move || {
            let mut adapter = adapter(mode);
            adapter.set_logger(logger.clone());
            let mut last_devices = Vec::new();
            let mut cache = ReceiptCache::new();
            let mut recent: VecDeque<CommandReceipt> = VecDeque::new();
            let initial = poll_snapshot(adapter.as_mut(), mode);
            log_device_events(&logger, &initial, &mut last_devices);
            *state.blocking_write() = initial;
            while !stop.load(Ordering::Relaxed) {
                match receive.recv_timeout(Duration::from_secs(1)) {
                    Ok(envelope) => {
                        logger.event(
                            LogLevel::Debug,
                            "control",
                            format!(
                                "Received {} for {}",
                                envelope.request.action.label(),
                                envelope.request.device_id
                            ),
                        );
                        let receipt = dispatch(
                            adapter.as_mut(),
                            mode,
                            &envelope.request,
                            envelope.reply.is_closed(),
                            &mut cache,
                            now_ms(),
                        );
                        logger.event(
                            if receipt.state == lightdriver_core::CommandState::Rejected {
                                LogLevel::Warning
                            } else {
                                LogLevel::Info
                            },
                            "control",
                            format!(
                                "{} · {:?} · {}",
                                receipt.action, receipt.state, receipt.detail
                            ),
                        );
                        if !recent.iter().any(|r| r.request_id == receipt.request_id) {
                            recent.push_front(receipt.clone());
                            recent.truncate(8);
                        }
                        // Reply before the next potentially slow hardware poll.
                        let _ = envelope.reply.send(receipt);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                let mut snapshot = poll_snapshot(adapter.as_mut(), mode);
                snapshot.command_receipts = recent.iter().cloned().collect();
                log_device_events(&logger, &snapshot, &mut last_devices);
                *state.blocking_write() = snapshot;
            }
        })?;
    Ok(send)
}

type DeviceLogState = Vec<(String, lightdriver_core::DeviceStatus, String, bool)>;
fn log_device_events(logger: &LogStore, snapshot: &Snapshot, previous: &mut DeviceLogState) {
    for device in &snapshot.devices {
        let entry = (
            device.id.clone(),
            device.status.clone(),
            device.detail.clone(),
            device.control.enabled,
        );
        if !previous.contains(&entry) {
            let level = match device.status {
                lightdriver_core::DeviceStatus::Error => LogLevel::Error,
                lightdriver_core::DeviceStatus::Disconnected => LogLevel::Warning,
                _ => LogLevel::Info,
            };
            logger.event(
                level,
                "discovery",
                format!(
                    "{} · {} · {}",
                    device.name,
                    device.status.label(),
                    device.detail
                ),
            );
        }
    }
    for (id, _, _, _) in previous.iter() {
        if !snapshot.devices.iter().any(|device| &device.id == id) {
            logger.event(
                LogLevel::Warning,
                "discovery",
                format!("Controller disconnected: {id}"),
            );
        }
    }
    *previous = snapshot
        .devices
        .iter()
        .map(|device| {
            (
                device.id.clone(),
                device.status.clone(),
                device.detail.clone(),
                device.control.enabled,
            )
        })
        .collect();
}
#[cfg(test)]
mod tests {
    use super::*;
    use lightdriver_core::{CommandState, ControlAction, Preset, PumpCurve};
    use lightdriver_devices::DemoAdapter;
    fn request(id: &str, action: ControlAction) -> ControlRequest {
        ControlRequest {
            request_id: id.into(),
            device_id: "demo-kraken-x3".into(),
            mode: Mode::Demo,
            expires_at_ms: now_ms() + 10_000,
            action,
        }
    }
    #[test]
    fn duplicate_and_conflicting_ids_cannot_reexecute_or_poison_receipts() {
        let mut demo = DemoAdapter::default();
        let mut cache = ReceiptCache::new();
        let r = request("enable", ControlAction::Enable);
        assert_eq!(
            dispatch(&mut demo, Mode::Demo, &r, false, &mut cache, now_ms()).state,
            CommandState::Simulated
        );
        let mut conflicting = r.clone();
        conflicting.action = ControlAction::Release;
        for _ in 0..100 {
            assert_eq!(
                dispatch(
                    &mut demo,
                    Mode::Demo,
                    &conflicting,
                    false,
                    &mut cache,
                    now_ms()
                )
                .state,
                CommandState::Rejected
            );
        }
        assert_eq!(cache.len(), 1);
        assert_eq!(
            dispatch(&mut demo, Mode::Demo, &r, false, &mut cache, now_ms()).state,
            CommandState::Simulated
        );
        assert!(demo.poll()[0].control.enabled);
        let r = request(
            "curve",
            ControlAction::PumpCurve(PumpCurve::example(Preset::Balanced).full_speed()),
        );
        assert_eq!(
            dispatch(&mut demo, Mode::Demo, &r, false, &mut cache, now_ms()).state,
            CommandState::Simulated
        );
        assert_eq!(demo.poll()[0].telemetry.as_ref().unwrap().pump_duty, 100);
    }
    #[test]
    fn canceled_expired_and_wrong_mode_requests_do_not_enable_control() {
        let mut demo = DemoAdapter::default();
        let mut cache = ReceiptCache::new();
        let r = request("canceled", ControlAction::Enable);
        assert_eq!(
            dispatch(&mut demo, Mode::Demo, &r, true, &mut cache, now_ms()).state,
            CommandState::Rejected
        );
        let mut r = request("expired", ControlAction::Enable);
        r.expires_at_ms = now_ms() - 1;
        assert_eq!(
            dispatch(&mut demo, Mode::Demo, &r, false, &mut cache, now_ms()).state,
            CommandState::Rejected
        );
        let mut r = request("wrong", ControlAction::Enable);
        r.mode = Mode::Live;
        assert_eq!(
            dispatch(&mut demo, Mode::Demo, &r, false, &mut cache, now_ms()).state,
            CommandState::Rejected
        );
        assert!(!demo.poll()[0].control.enabled);
    }
    #[test]
    fn full_cache_keeps_existing_commands_until_expiration() {
        let mut demo = DemoAdapter::default();
        let mut cache = ReceiptCache::new();
        for i in 0..CACHE_LIMIT {
            let r = request(&format!("r-{i}"), ControlAction::Enable);
            dispatch(&mut demo, Mode::Demo, &r, false, &mut cache, now_ms());
        }
        let r = request("release", ControlAction::Release);
        assert_eq!(
            dispatch(&mut demo, Mode::Demo, &r, false, &mut cache, now_ms()).state,
            CommandState::Rejected
        );
        assert!(demo.poll()[0].control.enabled);
        for (r, _) in &mut cache {
            r.expires_at_ms = now_ms() - 1;
        }
        assert_eq!(
            dispatch(&mut demo, Mode::Demo, &r, false, &mut cache, now_ms()).state,
            CommandState::Simulated
        );
        assert!(!demo.poll()[0].control.enabled);
    }
}
