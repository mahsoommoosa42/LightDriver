use lightdriver_core::{Draft, ProfileLibrary, RigConfiguration};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

pub fn default_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".data"))
        .join("LightDriver")
}

pub fn load(directory: &Path) -> Result<Draft, String> {
    let file = directory.join("kraken-draft.json");
    match fs::File::open(&file) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(32_769)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 32_768 {
                return Err("Saved draft is too large.".into());
            }
            let draft: Draft = serde_json::from_slice(&bytes)
                .map_err(|_| "Saved draft is not valid JSON.".to_owned())?;
            draft.validate()?;
            Ok(draft)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Draft::default()),
        Err(e) => Err(format!("Could not load the saved draft: {e}")),
    }
}

pub fn save(directory: &Path, draft: &Draft) -> Result<PathBuf, String> {
    draft.validate()?;
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let temporary = directory.join(format!("kraken-draft-{}.tmp", std::process::id()));
    let destination = directory.join("kraken-draft.json");
    let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(draft).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
    Ok(destination)
}

pub fn load_profiles(directory: &Path) -> Result<ProfileLibrary, String> {
    let library: ProfileLibrary = read_json(&directory.join("profiles.json"))?;
    library.validate()?;
    Ok(library)
}
pub fn save_profiles(directory: &Path, library: &ProfileLibrary) -> Result<PathBuf, String> {
    library.validate()?;
    save_json(directory, "profiles.json", library)
}
pub fn load_rig(directory: &Path) -> Result<RigConfiguration, String> {
    let config: RigConfiguration = read_json(&directory.join("wiring.json"))?;
    config.validate()?;
    Ok(config)
}
pub fn save_rig(directory: &Path, config: &RigConfiguration) -> Result<PathBuf, String> {
    config.validate()?;
    save_json(directory, "wiring.json", config)
}
pub fn import_draft(path: &Path) -> Result<Draft, String> {
    let file = fs::File::open(path).map_err(|e| format!("Could not open profile: {e}"))?;
    let mut bytes = Vec::new();
    file.take(262_145)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 262_144 {
        return Err("Profile exceeds 256 KiB.".into());
    }
    let draft: Draft =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid profile: {e}"))?;
    draft.validate()?;
    Ok(draft)
}
fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(262_145)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 262_144 {
                return Err("Configuration exceeds 256 KiB.".into());
            }
            serde_json::from_slice(&bytes).map_err(|e| format!("Invalid configuration: {e}"))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e.to_string()),
    }
}
pub fn save_json<T: serde::Serialize>(
    directory: &Path,
    name: &str,
    value: &T,
) -> Result<PathBuf, String> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let destination = directory.join(name);
    let temporary = directory.join(format!("{name}.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightdriver_core::{Preset, PumpCurve};

    #[test]
    fn profiles_and_user_wiring_roundtrip_without_device_defaults() {
        let directory =
            std::env::temp_dir().join(format!("lightdriver-profiles-test-{}", std::process::id()));
        assert!(load_rig(&directory).unwrap().lighting_routes.is_empty());
        let library = ProfileLibrary {
            schema_version: 1,
            profiles: vec![lightdriver_core::NamedProfile {
                name: "Quiet".into(),
                draft: Draft::default(),
            }],
        };
        save_profiles(&directory, &library).unwrap();
        assert_eq!(load_profiles(&directory).unwrap().profiles[0].name, "Quiet");
        let config = RigConfiguration {
            schema_version: 1,
            lighting_routes: vec![lightdriver_core::LightingRoute {
                name: "Case".into(),
                device_id: "motherboard".into(),
                provider: "vendor-sdk".into(),
                channel: "user-header".into(),
                board_identity: Some("Example board".into()),
            }],
        };
        save_rig(&directory, &config).unwrap();
        assert_eq!(
            load_rig(&directory).unwrap().lighting_routes[0].channel,
            "user-header"
        );
        let mut invalid = library.clone();
        invalid.profiles[0].draft.lighting_speed = 5;
        assert!(save_profiles(&directory, &invalid).is_err());
        assert_eq!(
            load_profiles(&directory).unwrap().profiles[0]
                .draft
                .lighting_speed,
            2
        );
        assert!(import_draft(&directory.join("missing.json")).is_err());
        let path = save_json(&directory, "export.json", &library.profiles[0].draft).unwrap();
        assert_eq!(
            import_draft(&path).unwrap().lighting_rgb,
            library.profiles[0].draft.lighting_rgb
        );
        for name in ["profiles.json", "wiring.json", "export.json"] {
            fs::remove_file(directory.join(name)).unwrap();
        }
        fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn legacy_profiles_gain_default_channels_but_invalid_effect_settings_are_rejected() {
        let legacy = serde_json::json!({"schema_version":1,"curve":PumpCurve::example(Preset::Balanced),
            "lighting_rgb":[83,221,202],"lighting_brightness":70});
        let mut draft: Draft = serde_json::from_value(legacy).unwrap();
        assert_eq!(
            draft.lighting_channel,
            lightdriver_core::LightingChannel::Both
        );
        assert_eq!(draft.lighting_speed, 2);
        assert!(draft.validate().is_ok());
        draft.lighting_effect = lightdriver_core::LightingEffect::Spectrum;
        assert!(draft.validate().is_err());
        draft.lighting_brightness = 100;
        assert!(draft.validate().is_ok());
    }
    #[test]
    fn replaces_valid_drafts_and_preserves_them_on_invalid_save() {
        let directory =
            std::env::temp_dir().join(format!("lightdriver-draft-test-{}", std::process::id()));
        let mut draft = Draft::default();
        save(&directory, &draft).unwrap();
        draft.curve = PumpCurve::example(Preset::Performance);
        save(&directory, &draft).unwrap();
        assert_eq!(load(&directory).unwrap().curve, draft.curve);
        draft.curve.points[0].duty = 19;
        assert!(save(&directory, &draft).is_err());
        assert_eq!(
            load(&directory).unwrap().curve,
            PumpCurve::example(Preset::Performance)
        );
        fs::write(directory.join("kraken-draft.json"), b"invalid JSON").unwrap();
        assert!(load(&directory).is_err());
        fs::remove_file(directory.join("kraken-draft.json")).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
