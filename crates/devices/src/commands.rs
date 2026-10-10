//! Independently encoded Kraken X3 wire commands. Hardware validation remains separate.
use lightdriver_core::{LightingChannel, LightingEffect, LightingSettings, PumpCurve};

pub fn curve_duty(curve: &PumpCurve, coolant: f32) -> u8 {
    if coolant >= 59.0 {
        return 100;
    }
    let points = &curve.points;
    if coolant <= f32::from(points[0].coolant_c) {
        return points[0].duty;
    }
    for pair in points.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if coolant <= f32::from(b.coolant_c) {
            let fraction =
                (coolant - f32::from(a.coolant_c)) / f32::from(b.coolant_c - a.coolant_c);
            return (f32::from(a.duty) + fraction * f32::from(b.duty - a.duty)).round() as u8;
        }
    }
    points.last().unwrap().duty
}
pub fn pump_report(curve: &PumpCurve) -> Result<[u8; 64], String> {
    curve.validate()?;
    let mut report = [0; 64];
    report[..4].copy_from_slice(&[0x72, 0x01, 0x00, 0x00]);
    for (i, temperature) in (20..=59).enumerate() {
        report[4 + i] = curve_duty(curve, temperature as f32);
    }
    Ok(report)
}
pub fn lighting_reports(settings: &LightingSettings) -> Result<Vec<[u8; 64]>, String> {
    settings.validate()?;
    let channels: &[u8] = match settings.channel {
        LightingChannel::Ring => &[2],
        LightingChannel::Logo => &[4],
        LightingChannel::Both => &[2, 4],
    };
    let (mode, speed, color_count, animation) = match settings.effect {
        LightingEffect::Static => (0, [0x32, 0], 1, 0),
        LightingEffect::Off => (0, [0x32, 0], 0, 0),
        LightingEffect::Breathing => (
            7,
            [[0x28, 0], [0x1e, 0], [0x14, 0], [0x0a, 0], [0x04, 0]][settings.speed as usize],
            1,
            8,
        ),
        LightingEffect::Spectrum => (
            2,
            [[0x5e, 1], [0x2c, 1], [0xfa, 0], [0x96, 0], [0x50, 0]][settings.speed as usize],
            1,
            0,
        ),
    };
    let scaled = settings
        .rgb
        .map(|v| ((u16::from(v) * u16::from(settings.brightness) + 50) / 100) as u8);
    Ok(channels
        .iter()
        .map(|&channel| {
            let mut report = [0; 64];
            report[..7].copy_from_slice(&[0x2a, 0x04, channel, channel, mode, speed[0], speed[1]]);
            if color_count > 0 && settings.effect != LightingEffect::Spectrum {
                report[7..10].copy_from_slice(&[scaled[1], scaled[0], scaled[2]]);
            }
            report[55..60].copy_from_slice(&[
                0,
                color_count,
                animation,
                if channel == 2 { 8 } else { 1 },
                3,
            ]);
            report
        })
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    use lightdriver_core::{CurvePoint, Preset};
    #[test]
    fn curve_interpolation_boundaries_and_critical_temperature() {
        let curve = PumpCurve {
            points: vec![
                CurvePoint {
                    coolant_c: 25,
                    duty: 20,
                },
                CurvePoint {
                    coolant_c: 35,
                    duty: 100,
                },
            ],
        };
        let report = pump_report(&curve).unwrap();
        assert_eq!(&report[..4], &[0x72, 1, 0, 0]);
        assert_eq!(report[4], 20);
        assert_eq!(report[14], 60);
        assert_eq!(report[19], 100);
        assert_eq!(report[43], 100);
        assert!(report[44..].iter().all(|&v| v == 0));
        let mut bad = PumpCurve::example(Preset::Balanced);
        bad.points[0].duty = 0;
        assert!(pump_report(&bad).is_err());
    }
    #[test]
    fn lighting_targets_are_only_ring_logo_and_rgb_is_grb_on_wire() {
        let settings = LightingSettings {
            channel: LightingChannel::Both,
            effect: LightingEffect::Static,
            rgb: [100, 200, 50],
            brightness: 50,
            speed: 2,
        };
        let reports = lighting_reports(&settings).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(&reports[0][..7], &[0x2a, 4, 2, 2, 0, 0x32, 0]);
        assert_eq!(&reports[0][7..10], &[100, 50, 25]);
        assert_eq!(&reports[0][55..60], &[0, 1, 0, 8, 3]);
        assert_eq!(&reports[1][55..60], &[0, 1, 0, 1, 3]);
    }
}
