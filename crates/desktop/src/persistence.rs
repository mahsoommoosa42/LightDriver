use lightdriver_core::Draft;
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

#[cfg(test)]
mod tests {
    use super::*;
    use lightdriver_core::{Preset, PumpCurve};

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
