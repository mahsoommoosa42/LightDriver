//! Explicit bounded tachometer check for development/support. No firmware getter.
use crate::rpm;
use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{Duration, SystemTime},
};
struct Options {
    demo: bool,
    samples: u8,
    output: Option<PathBuf>,
}
fn parse(args: &[OsString]) -> Result<Options, String> {
    let mut options = Options {
        demo: false,
        samples: 3,
        output: None,
    };
    let mut check = false;
    let mut sample_option = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--rpm-check" && !check {
            check = true;
        } else if arg == "--demo" && !options.demo {
            options.demo = true;
        } else if arg == "--samples" && !sample_option {
            sample_option = true;
            options.samples = iter
                .next()
                .and_then(|v| v.to_str())
                .and_then(|v| v.parse().ok())
                .filter(|v| (1..=5).contains(v))
                .ok_or("RPM check requires 1–5 samples")?;
        } else if arg == "--output" && options.output.is_none() {
            let path = iter
                .next()
                .filter(|v| !v.is_empty())
                .ok_or("Missing output path")?;
            options.output = Some(PathBuf::from(path));
        } else {
            return Err(format!(
                "Unknown or repeated RPM check argument: {}",
                arg.to_string_lossy()
            ));
        }
    }
    if !check {
        return Err("Explicit --rpm-check required".into());
    }
    Ok(options)
}
fn inspect(options: &Options) -> (serde_json::Value, bool) {
    let mut samples = Vec::new();
    let mut success = true;
    for index in 0..options.samples {
        let started = std::time::Instant::now();
        let result = if options.demo {
            Ok(rpm::simulated())
        } else {
            rpm::sample()
        };
        let timestamp_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|v| v.as_millis())
            .unwrap_or(0);
        match result {
            Ok(snapshot) => {
                samples.push(serde_json::json!({
                    "sample": index + 1, "timestamp_unix_ms": timestamp_ms,
                    "duration_ms": started.elapsed().as_millis(),
                    "source": snapshot.source,
                    "readings": snapshot.readings.iter().map(|r| serde_json::json!({
                        "header": r.header, "channel": r.channel, "rpm": r.rpm, "detail": r.detail
                    })).collect::<Vec<_>>()
                }));
            }
            Err(error) => {
                samples.push(serde_json::json!({ "sample": index + 1, "timestamp_unix_ms": timestamp_ms, "error": error }));
                success = false;
                break; // No blind hardware retries after failure.
            }
        }
        if index + 1 < options.samples {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    (
        serde_json::json!({
            "schema_version": 1, "operation": "read_only_motherboard_rpm",
            "simulated": options.demo, "status": if success { "read" } else { "failed" },
            "firmware_read": false, "fan_settings_written": false, "samples": samples
        }),
        success,
    )
}
pub fn run(args: &[OsString]) -> Result<i32, String> {
    let options = parse(args)?;
    // Establish a new output before any hardware operation. Never replace an existing report.
    let mut file = options
        .output
        .as_ref()
        .map(|path| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| format!("Cannot create RPM report: {e}"))
        })
        .transpose()?;
    let (report, success) = inspect(&options);
    let mut bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    if let Some(file) = &mut file {
        file.write_all(&bytes).map_err(|e| e.to_string())?;
    } else {
        std::io::stdout()
            .write_all(&bytes)
            .map_err(|e| e.to_string())?;
    }
    Ok(if success { 0 } else { 1 })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn checks_are_explicit_bounded_and_reject_repeated_arguments() {
        for values in [
            vec![],
            vec!["--samples", "1"],
            vec!["--rpm-check", "--samples", "0"],
            vec!["--rpm-check", "--samples", "6"],
            vec!["--rpm-check", "--rpm-check"],
            vec!["--rpm-check", "--output"],
            vec!["--rpm-check", "--demo", "--demo"],
            vec!["--rpm-check", "--firmware-write"],
        ] {
            assert!(parse(&args(&values)).is_err());
        }
        let options = parse(&args(&["--rpm-check", "--samples", "1", "--demo"])).unwrap();
        let (report, success) = inspect(&options);
        assert!(success);
        assert_eq!(report["samples"].as_array().unwrap().len(), 1);
        assert_eq!(report["simulated"], true);
        assert_eq!(report["firmware_read"], false);
        assert_eq!(report["fan_settings_written"], false);
    }
    #[test]
    fn an_existing_output_is_preserved_before_any_sample() {
        let path =
            std::env::temp_dir().join(format!("lightdriver-rpm-check-{}.json", std::process::id()));
        std::fs::write(&path, b"keep original").unwrap();
        let args = vec![
            "--rpm-check".into(),
            "--demo".into(),
            "--output".into(),
            path.clone().into_os_string(),
        ];
        assert!(run(&args).unwrap_err().contains("Cannot create"));
        assert_eq!(std::fs::read(&path).unwrap(), b"keep original");
        std::fs::remove_file(path).unwrap();
    }
}
