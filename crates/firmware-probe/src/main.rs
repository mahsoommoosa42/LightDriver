//! Standalone research tool. One fixed firmware-variable read; no setters or drivers.
mod report;
#[cfg(windows)]
mod windows;

use std::{ffi::OsString, fs::OpenOptions, io::Write, path::PathBuf, process::ExitCode};

const HELP: &str = "LightDriver firmware probe (read only)
Usage: lightdriver-firmware-probe --inspect [--output <new-json-file>]

Reads only GcSensorVarName in its fixed vendor namespace.
Reports boot mode, process privilege, variable attributes, size and header.
No raw payload export, firmware writes, driver access or automatic elevation.
Windows may require Run as administrator to read this variable.
Exit codes: 0 = variable read; 1 = unavailable/failed; 2 = usage/export error.
";

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Option<PathBuf>, String> {
    let mut args = args.into_iter();
    let mut inspect = false;
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--inspect") if !inspect => inspect = true,
            Some("--output") if output.is_none() => {
                output = Some(PathBuf::from(args.next().ok_or("--output needs a path")?));
            }
            _ => return Err("Unknown or repeated argument; use --help".into()),
        }
    }
    if !inspect {
        return Err("--inspect is required; no firmware read was performed".into());
    }
    Ok(output)
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.is_empty() || args == [OsString::from("--help")] || args == [OsString::from("-h")] {
        print!("{HELP}");
        return Ok(ExitCode::SUCCESS);
    }
    let output = parse_args(args)?;
    #[cfg(windows)]
    let report = windows::inspect();
    #[cfg(not(windows))]
    let report = report::ProbeReport::unsupported();
    let mut json = serde_json::to_vec_pretty(&report)?;
    json.push(b'\n');
    if let Some(path) = output {
        // A prior capture is evidence: never silently overwrite it.
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?
            .write_all(&json)?;
    } else {
        std::io::stdout().lock().write_all(&json)?;
    }
    Ok(
        if report.variable.status == "read" && report.privilege_restoration.error.is_none() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        },
    )
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspection_requires_explicit_fixed_read_and_rejects_custom_variables() {
        assert!(parse_args([OsString::from("--output"), OsString::from("report.json")]).is_err());
        assert!(parse_args([OsString::from("--inspect"), OsString::from("--variable")]).is_err());
        assert!(parse_args([OsString::from("--inspect"), OsString::from("--inspect")]).is_err());
        assert_eq!(parse_args([OsString::from("--inspect")]).unwrap(), None);
    }
}
