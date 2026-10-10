//! Explicit private-file storage. No automatic upload or firmware operation.
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, io::Write, path::Path};
pub fn sha256(data: &[u8]) -> String {
    format!("{:X}", Sha256::digest(data))
}
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Cannot create a new file: {e}"))?;
    file.write_all(bytes)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("File save failed; partial file may remain: {e}"))
}
