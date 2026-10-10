//! Explicit private capture and offline comparison. No firmware setter or driver call.
use lightdriver_firmware_probe::{
    inspect_snapshot,
    snapshot::{Snapshot, write_new},
};
use std::{ffi::OsString, path::PathBuf};
enum Operation {
    Capture {
        output: PathBuf,
        demo: bool,
    },
    Compare {
        baseline: PathBuf,
        current: PathBuf,
        output: PathBuf,
    },
}
fn parse(args: &[OsString]) -> Result<Operation, String> {
    let mut capture = false;
    let mut compare = None;
    let mut output = None;
    let mut demo = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--firmware-snapshot" && !capture && compare.is_none() {
            capture = true;
        } else if arg == "--compare-firmware" && !capture && compare.is_none() {
            let base = iter
                .next()
                .filter(|p| !p.is_empty())
                .ok_or("Missing baseline snapshot")?;
            let current = iter
                .next()
                .filter(|p| !p.is_empty())
                .ok_or("Missing current snapshot")?;
            compare = Some((PathBuf::from(base), PathBuf::from(current)));
        } else if arg == "--output" && output.is_none() {
            output = Some(PathBuf::from(
                iter.next()
                    .filter(|p| !p.is_empty())
                    .ok_or("Missing output path")?,
            ));
        } else if arg == "--demo" && !demo {
            demo = true;
        } else {
            return Err(format!(
                "Unknown or repeated firmware-check option: {}",
                arg.to_string_lossy()
            ));
        }
    }
    let output = output
        .ok_or("Explicit --output is required; a private payload is never printed to stdout")?;
    if capture {
        Ok(Operation::Capture { output, demo })
    } else if let Some((baseline, current)) = compare {
        if demo {
            return Err("--demo applies only to capture".into());
        }
        Ok(Operation::Compare {
            baseline,
            current,
            output,
        })
    } else {
        Err("Explicit --firmware-snapshot or --compare-firmware required".into())
    }
}
pub fn run(args: &[OsString]) -> Result<i32, String> {
    match parse(args)? {
        Operation::Capture { output, demo } => {
            if output.exists() {
                return Err("Snapshot destination already exists; refusing overwrite".into());
            }
            let snapshot = if demo {
                Snapshot::synthetic()
            } else {
                let read = inspect_snapshot();
                read.snapshot.ok_or_else(|| {
                    let errors = [
                        &read.report.firmware,
                        &read.report.privilege,
                        &read.report.privilege_restoration,
                    ]
                    .iter()
                    .filter_map(|s| s.error.as_ref())
                    .map(|e| format!("{}: Win32 {}", e.operation, e.win32_code))
                    .chain(
                        read.report
                            .variable
                            .error
                            .iter()
                            .map(|e| format!("{}: Win32 {}", e.operation, e.win32_code)),
                    )
                    .collect::<Vec<_>>()
                    .join("; ");
                    format!(
                        "No reference snapshot: {}. {errors}",
                        read.report.variable.status
                    )
                })?
            };
            snapshot.save_private(&output)?;
        }
        Operation::Compare {
            baseline,
            current,
            output,
        } => {
            // Imports and comparison are offline; no hardware API is called.
            let base = Snapshot::load_private(&baseline)?;
            let current = Snapshot::load_private(&current)?;
            let comparison = base.compare(&current)?;
            let bytes = serde_json::to_vec_pretty(&comparison).map_err(|e| e.to_string())?;
            write_new(&output, &bytes)?;
        }
    }
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }
    #[test]
    fn parser_requires_explicit_mode_destination_and_bounded_arguments() {
        assert!(parse(&args(&["--firmware-snapshot", "--demo"])).is_err());
        assert!(
            parse(&args(&[
                "--firmware-snapshot",
                "--output",
                "x",
                "--output",
                "y"
            ]))
            .is_err()
        );
        assert!(
            parse(&args(&[
                "--firmware-snapshot",
                "--compare-firmware",
                "x",
                "y",
                "--output",
                "z"
            ]))
            .is_err()
        );
        assert!(
            parse(&args(&[
                "--compare-firmware",
                "x",
                "y",
                "--output",
                "z",
                "--demo"
            ]))
            .is_err()
        );
        assert!(parse(&args(&["--firmware-snapshot", "--demo", "--output", "x"])).is_ok());
        assert!(parse(&args(&["--compare-firmware", "x", "y", "--output", "z"])).is_ok());
    }
    #[test]
    fn demo_capture_and_offline_comparison_never_need_hardware_and_refuse_overwrite() {
        let dir = std::env::temp_dir().join(format!(
            "lightdriver-firmware-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let a = dir.join("a.json");
        let b = dir.join("b.json");
        let report = dir.join("report.json");
        for path in [&a, &b] {
            run(&[
                OsString::from("--firmware-snapshot"),
                OsString::from("--demo"),
                OsString::from("--output"),
                path.as_os_str().to_owned(),
            ])
            .unwrap();
        }
        assert!(
            run(&[
                OsString::from("--firmware-snapshot"),
                OsString::from("--output"),
                a.as_os_str().to_owned()
            ])
            .is_err()
        );
        run(&[
            OsString::from("--compare-firmware"),
            a.as_os_str().to_owned(),
            b.as_os_str().to_owned(),
            OsString::from("--output"),
            report.as_os_str().to_owned(),
        ])
        .unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
        assert_eq!(json["identical"], true);
        assert_eq!(json["origin"], "synthetic");
        assert!(json.get("payload_hex").is_none());
        for path in [&a, &b, &report] {
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}
