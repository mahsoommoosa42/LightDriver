//! A new immutable initial snapshot for each app session. No firmware writes.
use super::snapshot::{Origin, Snapshot, sha256};
use std::path::{Path, PathBuf};
#[derive(Debug, Clone)]
pub struct Recovery {
    pub snapshot: Snapshot,
    pub path: PathBuf,
}
/// Called after the first successful startup/manual read; never for demo.
pub fn save_initial(snapshot: Snapshot) -> Result<Recovery, String> {
    if snapshot.origin != Origin::Live {
        return Err("Automatic disk recovery is live-only".into());
    }
    let dir = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or("LOCALAPPDATA is unavailable; choose a private backup path manually")?
        .join("LightDriver")
        .join("FirmwareBackups");
    let inventory = serde_json::to_vec(&snapshot.identity).map_err(|e| e.to_string())?;
    let identity_key = sha256(&inventory);
    let saved_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_nanos())
        .unwrap_or(0);
    let file = format!(
        "initial-{}-{}-{}-{saved_at}.private.json",
        &identity_key[..16],
        snapshot.captured_unix_ms,
        std::process::id()
    );
    save_initial_at(snapshot, &dir, &file)
}
fn save_initial_at(snapshot: Snapshot, dir: &Path, file: &str) -> Result<Recovery, String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Cannot create private recovery folder: {e}"))?;
    let path = dir.join(file);
    snapshot.save_private(&path)?;
    let recovered = Snapshot::load_private(&path)?;
    if !snapshot.compare(&recovered)?.identical
        || snapshot.captured_unix_ms != recovered.captured_unix_ms
    {
        return Err("Recovery file readback differs; recovery is not ready".into());
    }
    Ok(Recovery {
        snapshot: recovered,
        path,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_snapshot_is_saved_verified_and_never_overwritten_by_later_reads() {
        let original = Snapshot::synthetic();
        let dir = std::env::temp_dir().join(format!(
            "fan-lab-initial-{}-{}",
            std::process::id(),
            original.captured_unix_ms
        ));
        let saved = save_initial_at(original.clone(), &dir, "initial.private.json").unwrap();
        let mut later = original.clone();
        later.captured_unix_ms += 1000;
        assert!(save_initial_at(later, &dir, "initial.private.json").is_err());
        let recovered = Snapshot::load_private(&saved.path).unwrap();
        assert_eq!(recovered.captured_unix_ms, original.captured_unix_ms);
        assert!(original.compare(&recovered).unwrap().identical);
        assert!(save_initial(original).is_err()); // Demo cannot trigger automatic app-data writes.
        std::fs::remove_file(saved.path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
