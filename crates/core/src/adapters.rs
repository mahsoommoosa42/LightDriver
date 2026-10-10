//! Shared adapter contracts; no operating-system or vendor hardware access.
use crate::{CommandReceipt, ControlRequest, Device, LogStore};

pub trait DeviceAdapter: Send {
    fn poll(&mut self) -> Vec<Device>;
    fn set_logger(&mut self, _logger: LogStore) {}
    fn control(&mut self, request: &ControlRequest) -> CommandReceipt {
        CommandReceipt::rejected(request, "This adapter does not expose device control.")
    }
}
