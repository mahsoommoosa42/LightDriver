//! Typed, timestamped sensor data. Unavailable is distinct from a measured zero.
use crate::STALE_AFTER_MS;
use serde::{Deserialize, Serialize};

pub const SENSOR_LIMIT: usize = 48;
pub const SENSOR_HISTORY_LIMIT: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    Celsius,
    Percent,
    Watts,
    Rpm,
    Mhz,
    Gib,
}
impl Unit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Celsius => "°C",
            Self::Percent => "%",
            Self::Watts => "W",
            Self::Rpm => "RPM",
            Self::Mhz => "MHz",
            Self::Gib => "GiB",
        }
    }
    pub fn accepts(self, value: f64) -> bool {
        value.is_finite()
            && match self {
                Self::Celsius => (-50.0..=200.0).contains(&value),
                Self::Percent => (0.0..=100.0).contains(&value),
                Self::Watts => (0.0..=2000.0).contains(&value),
                Self::Rpm => (0.0..=30_000.0).contains(&value),
                Self::Mhz => (0.0..=20_000.0).contains(&value),
                Self::Gib => (0.0..=1_000_000.0).contains(&value),
            }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SensorStatus {
    Available,
    Unavailable,
    Error,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorSample {
    pub at_ms: u64,
    pub value: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sensor {
    pub id: String,
    pub group: String,
    pub label: String,
    pub unit: Unit,
    pub source: String,
    pub basic: bool,
    pub status: SensorStatus,
    pub detail: String,
    pub sample: Option<SensorSample>,
    pub history: Vec<SensorSample>,
}
impl Sensor {
    pub fn fresh_value_at(&self, now: u64) -> Option<f64> {
        self.sample
            .as_ref()
            .filter(|s| {
                self.status == SensorStatus::Available
                    && s.at_ms <= now
                    && now - s.at_ms <= STALE_AFTER_MS
                    && self.unit.accepts(s.value)
            })
            .map(|s| s.value)
    }
    pub fn range(&self) -> Option<(f64, f64)> {
        let mut values = self
            .history
            .iter()
            .map(|s| s.value)
            .filter(|v| self.unit.accepts(*v));
        let first = values.next()?;
        Some(values.fold((first, first), |(min, max), v| (min.min(v), max.max(v))))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub name: String,
    pub detail: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Diagnostics {
    pub sensors: Vec<Sensor>,
    pub providers: Vec<Provider>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_stale_future_and_invalid_are_not_live_values() {
        let mut s = Sensor {
            id: "test".into(),
            group: "GPU".into(),
            label: "Power".into(),
            unit: Unit::Watts,
            source: "test".into(),
            basic: true,
            status: SensorStatus::Available,
            detail: String::new(),
            sample: Some(SensorSample {
                at_ms: 1000,
                value: 0.0,
            }),
            history: vec![],
        };
        assert_eq!(s.fresh_value_at(1000), Some(0.0));
        assert_eq!(s.fresh_value_at(6001), None);
        assert_eq!(s.fresh_value_at(999), None);
        s.status = SensorStatus::Unavailable;
        assert_eq!(s.fresh_value_at(1000), None);
        s.status = SensorStatus::Available;
        s.sample.as_mut().unwrap().value = f64::NAN;
        assert_eq!(s.fresh_value_at(1000), None);
        assert!(!Unit::Percent.accepts(101.0));
        assert!(!Unit::Watts.accepts(-1.0));
    }
}
