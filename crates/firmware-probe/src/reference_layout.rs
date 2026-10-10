//! Partial read-only observations for the analyzed F41c image.
//! These fields cannot establish a complete curve codec or writable support.

pub const PAYLOAD_LENGTH: usize = 569;
pub const BOARD_IMAGE_ID: &[u8; 8] = b"8AMTS010";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Header {
    CpuFan,
    CpuOpt,
    SysFan1,
    SysFan2,
}
impl Header {
    pub const ALL: [Self; 4] = [Self::CpuFan, Self::CpuOpt, Self::SysFan1, Self::SysFan2];
    pub const CASE: [Self; 2] = [Self::SysFan1, Self::SysFan2];
    fn block(self) -> usize {
        match self {
            Self::CpuFan => 0x0a,
            Self::CpuOpt => 0x37,
            Self::SysFan1 => 0x64,
            Self::SysFan2 => 0x91,
        }
    }
    fn tail(self) -> (usize, usize) {
        match self {
            Self::CpuFan => (0x20e, 0x21d),
            Self::CpuOpt => (0x20f, 0x21e),
            Self::SysFan1 => (0x214, 0x21f),
            Self::SysFan2 => (0x215, 0x220),
        }
    }
}
impl std::fmt::Display for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CpuFan => "CPU_FAN",
            Self::CpuOpt => "CPU_OPT",
            Self::SysFan1 => "SYS_FAN1",
            Self::SysFan2 => "SYS_FAN2",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub temperature: u8,
    /// A stored byte whose complete hardware role is not established.
    pub auxiliary_raw: u8,
    /// Sign-magnitude coefficient delta, not a temperature/duty pair.
    pub coefficient_delta: i32,
    pub coefficient_raw: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderSettings {
    pub header: Header,
    pub mode_raw: u8,
    pub off_temperature: u8,
    pub start_temperature: u8,
    pub full_temperature: u8,
    pub start_level_raw: u8,
    pub base_coefficient_raw: u16,
    pub interval_raw: u8,
    pub source_raw: u8,
    pub segments: [Segment; 3],
    pub pwm_dc_raw: u8,
    pub fan_stop_raw: u8,
}
impl HeaderSettings {
    pub fn mode_label(self) -> &'static str {
        match self.mode_raw {
            0 => "Normal",
            1 => "Silent",
            2 => "Manual curve",
            3 => "Full speed",
            _ => "Unknown",
        }
    }
    pub fn accumulated_coefficients(self) -> [i32; 4] {
        let mut values = [i32::from(self.base_coefficient_raw); 4];
        for (index, segment) in self.segments.iter().enumerate() {
            values[index + 1] = values[index] + segment.coefficient_delta;
        }
        values
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Length,
    Header,
    Attributes,
}

/// Strict format check for reading identified fields only.
/// The same header can span firmware versions; this is deliberately not
/// an identity gate for writes. All data and attributes remain unmodified.
pub fn decode(data: &[u8], attributes: u32) -> Result<[HeaderSettings; 4], DecodeError> {
    if data.len() != PAYLOAD_LENGTH {
        return Err(DecodeError::Length);
    }
    if data.get(..8) != Some(BOARD_IMAGE_ID.as_slice())
        || u16::from_le_bytes([data[8], data[9]]) as usize != PAYLOAD_LENGTH
    {
        return Err(DecodeError::Header);
    }
    if attributes != 7 {
        return Err(DecodeError::Attributes);
    }
    Ok(Header::ALL.map(|header| {
        let b = header.block();
        let word = |offset| u16::from_le_bytes([data[offset], data[offset + 1]]);
        let (pwm_dc, stop) = header.tail();
        HeaderSettings {
            header,
            mode_raw: data[b],
            off_temperature: data[b + 1],
            start_temperature: data[b + 2],
            full_temperature: data[b + 3],
            start_level_raw: data[b + 4],
            base_coefficient_raw: word(b + 5),
            interval_raw: data[b + 7],
            source_raw: data[b + 8],
            segments: std::array::from_fn(|i| {
                let offset = b + 9 + i * 4;
                let raw = word(offset + 2);
                Segment {
                    temperature: data[offset],
                    auxiliary_raw: data[offset + 1],
                    coefficient_delta: sign_magnitude(raw),
                    coefficient_raw: raw,
                }
            }),
            pwm_dc_raw: data[pwm_dc],
            fan_stop_raw: data[stop],
        }
    }))
}

fn sign_magnitude(value: u16) -> i32 {
    let magnitude = i32::from(value & 0x7fff);
    if value & 0x8000 != 0 {
        -magnitude
    } else {
        magnitude
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldChange {
    pub offset: usize,
    pub before: u8,
    pub after: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LevelPreview {
    pub requested_percent: u8,
    pub level_raw: u8,
    pub representable_percent: f32,
    /// Identified fields only, not an encoded firmware payload.
    pub partial_changes: Vec<FieldChange>,
    pub start_temperature: u8,
    pub full_temperature: u8,
}

/// Proposes a constant level between existing start/full thresholds.
/// Off/full-speed thresholds and unrelated bytes would need preservation.
/// This does NOT encode a complete curve or grant write permission.
pub fn preview(settings: HeaderSettings, percent: u8) -> Result<LevelPreview, &'static str> {
    if !(20..=100).contains(&percent) {
        return Err("Preview range is 20–100%");
    }
    if !Header::CASE.contains(&settings.header) {
        return Err("First experiment is case headers only");
    }
    if settings.mode_raw != 2 {
        return Err("Preview requires stored Manual mode; preset synthesis is incomplete");
    }
    if settings.off_temperature > settings.start_temperature
        || settings.start_temperature >= settings.full_temperature
        || settings.segments.iter().any(|s| {
            s.temperature < settings.start_temperature || s.temperature > settings.full_temperature
        })
        || settings
            .segments
            .windows(2)
            .any(|pair| pair[0].temperature >= pair[1].temperature)
    {
        return Err("Stored thresholds need investigation before a preview");
    }
    let raw = ((u16::from(percent) * 255 + 50) / 100) as u8;
    let b = settings.header.block();
    let mut changes = Vec::new();
    let mut add = |offset, before, after| {
        if before != after {
            changes.push(FieldChange {
                offset,
                before,
                after,
            });
        }
    };
    add(b, settings.mode_raw, 2);
    add(b + 4, settings.start_level_raw, raw);
    for (i, byte) in settings
        .base_coefficient_raw
        .to_le_bytes()
        .iter()
        .enumerate()
    {
        add(b + 5 + i, *byte, 0);
    }
    for (i, segment) in settings.segments.iter().enumerate() {
        for (j, byte) in segment.coefficient_raw.to_le_bytes().iter().enumerate() {
            add(b + 11 + i * 4 + j, *byte, 0);
        }
    }
    Ok(LevelPreview {
        requested_percent: percent,
        level_raw: raw,
        representable_percent: f32::from(raw) * 100.0 / 255.0,
        partial_changes: changes,
        start_temperature: settings.start_temperature,
        full_temperature: settings.full_temperature,
    })
}

pub fn synthetic_example() -> [HeaderSettings; 4] {
    let mut data = [0; PAYLOAD_LENGTH];
    data[..8].copy_from_slice(BOARD_IMAGE_ID);
    data[8..10].copy_from_slice(&(PAYLOAD_LENGTH as u16).to_le_bytes());
    for header in Header::ALL {
        let b = header.block();
        data[b] = 2;
        data[b + 2] = 25;
        data[b + 3] = 85;
        data[b + 4] = 96;
        data[b + 5] = 16;
        data[b + 7] = 3;
        data[b + 8] = 2;
        for (i, temp) in [40, 55, 70].iter().enumerate() {
            data[b + 9 + i * 4] = *temp;
            data[b + 11 + i * 4] = 4;
        }
        let (pwm_dc, _) = header.tail();
        data[pwm_dc] = 2;
    }
    decode(&data, 7).expect("synthetic reference format")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> [u8; PAYLOAD_LENGTH] {
        let mut bytes = [0xa5; PAYLOAD_LENGTH];
        bytes[..8].copy_from_slice(BOARD_IMAGE_ID);
        bytes[8..10].copy_from_slice(&(PAYLOAD_LENGTH as u16).to_le_bytes());
        bytes
    }
    #[test]
    fn format_gate_rejects_truncation_other_headers_and_additional_attributes() {
        let mut bytes = fixture();
        for length in 0..PAYLOAD_LENGTH {
            assert_eq!(decode(&bytes[..length], 7), Err(DecodeError::Length));
        }
        assert_eq!(decode(&[0; PAYLOAD_LENGTH], 7), Err(DecodeError::Header));
        assert_eq!(decode(&bytes, 0x27), Err(DecodeError::Attributes));
        bytes[0] = b'9';
        assert_eq!(decode(&bytes, 7), Err(DecodeError::Header));
    }
    #[test]
    fn observed_offsets_decode_independently_for_every_header_without_mutation() {
        let mut bytes = fixture();
        for (index, header) in Header::ALL.into_iter().enumerate() {
            let b = header.block();
            bytes[b..b + 9].copy_from_slice(&[2, 0, 20, 90, index as u8 + 100, 28, 0, 3, 2]);
            for (i, temp) in [30, 40, 50].into_iter().enumerate() {
                let p = b + 9 + i * 4;
                bytes[p] = temp;
                bytes[p + 1] = 0x7c;
                let coefficient: u16 = if i == 1 { 0x8004 } else { 4 };
                bytes[p + 2..p + 4].copy_from_slice(&coefficient.to_le_bytes());
            }
            let (pwm_dc, stop) = header.tail();
            bytes[pwm_dc] = index as u8;
            bytes[stop] = (index % 2) as u8;
        }
        let before = bytes;
        let settings = decode(&bytes, 7).unwrap();
        for (index, value) in settings.into_iter().enumerate() {
            assert_eq!(value.header, Header::ALL[index]);
            assert_eq!(value.start_level_raw, index as u8 + 100);
            assert_eq!(value.pwm_dc_raw, index as u8);
            assert_eq!(value.fan_stop_raw, (index % 2) as u8);
            assert_eq!(value.segments[1].coefficient_delta, -4);
            assert_eq!(value.segments[1].auxiliary_raw, 0x7c);
            assert_eq!(value.accumulated_coefficients(), [28, 32, 28, 32]);
        }
        assert_eq!(bytes, before);
    }
    #[test]
    fn previews_report_quantization_and_restrict_changes_to_selected_case_header() {
        let settings = synthetic_example();
        for target in [settings[2], settings[3]] {
            let p = preview(target, 75).unwrap();
            assert_eq!(p.level_raw, 191);
            assert!((p.representable_percent - 74.90196).abs() < 0.0001);
            let b = target.header.block();
            assert!(p.partial_changes.iter().all(|c| c.offset == b
                || c.offset == b + 4
                || c.offset == b + 5
                || c.offset == b + 6
                || (0..3).any(|i| c.offset == b + 11 + 4 * i || c.offset == b + 12 + 4 * i)));
            assert_eq!(p.start_temperature, 25);
            assert_eq!(p.full_temperature, 85);
        }
        assert!(preview(settings[0], 75).is_err());
        assert!(preview(settings[1], 75).is_err());
        assert!(preview(settings[2], 0).is_err());
        assert!(preview(settings[2], 101).is_err());
        let mut unknown = settings[2];
        unknown.mode_raw = 255;
        assert!(preview(unknown, 75).is_err());
        unknown = settings[2];
        unknown.segments[1].temperature = unknown.segments[0].temperature;
        assert!(preview(unknown, 75).is_err());
    }
    #[test]
    fn sign_magnitude_handles_negative_zero_and_largest_magnitude() {
        assert_eq!(sign_magnitude(0x8000), 0);
        assert_eq!(sign_magnitude(0x8001), -1);
        assert_eq!(sign_magnitude(0xffff), -32767);
        assert_eq!(sign_magnitude(0x7fff), 32767);
    }
}
