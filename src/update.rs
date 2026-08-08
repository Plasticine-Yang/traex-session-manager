use std::process::{Command, Output};

use anyhow::{Context, Result};

const OWNER_REPO: &str = "Plasticine-Yang/traex-session-manager";

pub fn self_update(check_only: bool) -> Result<()> {
    self_update_with(check_only, env!("CARGO_PKG_VERSION"), run_command)
}

fn self_update_with(
    check_only: bool,
    current_version: &str,
    runner: impl Fn(&str, &[&str]) -> Result<Output>,
) -> Result<()> {
    let latest_url = format!("https://github.com/{OWNER_REPO}/releases/latest");
    let output = runner(
        "curl",
        &[
            "-fsSL",
            "-o",
            "/dev/null",
            "-w",
            "%{url_effective}",
            &latest_url,
        ],
    )
    .context("failed to check the latest release")?;
    if !output.status.success() {
        anyhow::bail!(
            "failed to check the latest release: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let effective_url = String::from_utf8(output.stdout).context("release URL was not UTF-8")?;
    let latest_version = effective_url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .and_then(|tag| tag.strip_prefix('v'))
        .context("latest release URL did not end in a vX.Y.Z tag")?;

    match compare_versions(current_version, latest_version)? {
        std::cmp::Ordering::Equal => {
            println!("tsm {current_version} is already up to date");
            return Ok(());
        }
        std::cmp::Ordering::Greater => {
            println!(
                "tsm {current_version} is newer than latest release {latest_version}; not downgrading"
            );
            return Ok(());
        }
        std::cmp::Ordering::Less if check_only => {
            println!("update available: {current_version} -> {latest_version}");
            return Ok(());
        }
        std::cmp::Ordering::Less => {}
    }

    let install_url = format!("https://raw.githubusercontent.com/{OWNER_REPO}/main/install.sh");
    let script = format!(
        "set -eu
script=${{TMPDIR:-/tmp}}/tsm-update.$$
trap 'rm -f \"$script\"' EXIT HUP INT TERM
curl -fsSL '{}' -o \"$script\"
sh \"$script\"",
        install_url
    );
    let output = runner("sh", &["-c", &script]).context("failed to run installer")?;
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        anyhow::bail!("update failed");
    }
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn run_command(program: &str, args: &[&str]) -> Result<Output> {
    Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute {program}"))
}

fn compare_versions(left: &str, right: &str) -> Result<std::cmp::Ordering> {
    Ok(parse_version(left)?.cmp(&parse_version(right)?))
}

fn parse_version(version: &str) -> Result<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = parse_part(parts.next(), version)?;
    let minor = parse_part(parts.next(), version)?;
    let patch = parse_part(parts.next(), version)?;
    if parts.next().is_some() {
        anyhow::bail!("invalid release version: {version}");
    }
    Ok((major, minor, patch))
}

fn parse_part(part: Option<&str>, version: &str) -> Result<u64> {
    part.context("missing release version component")?
        .parse()
        .with_context(|| format!("invalid release version: {version}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::os::unix::process::ExitStatusExt;

    fn output(stdout: &str) -> Output {
        Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn check_only_does_not_run_the_installer_when_update_exists() {
        let calls = RefCell::new(Vec::new());
        self_update_with(true, "0.1.0", |program, args| {
            calls
                .borrow_mut()
                .push(format!("{program} {}", args.join(" ")));
            Ok(output(
                "https://github.com/Plasticine-Yang/traex-session-manager/releases/tag/v0.2.0",
            ))
        })
        .unwrap();

        assert_eq!(calls.borrow().len(), 1);
        assert!(calls.borrow()[0].starts_with("curl "));
    }

    #[test]
    fn current_or_newer_version_never_runs_the_installer() {
        for current in ["0.2.0", "0.3.0"] {
            let calls = RefCell::new(0);
            self_update_with(false, current, |_, _| {
                *calls.borrow_mut() += 1;
                Ok(output(
                    "https://github.com/Plasticine-Yang/traex-session-manager/releases/tag/v0.2.0",
                ))
            })
            .unwrap();
            assert_eq!(*calls.borrow(), 1);
        }
    }

    #[test]
    fn older_version_runs_the_shared_installer() {
        let calls = RefCell::new(Vec::new());
        self_update_with(false, "0.1.0", |program, args| {
            calls
                .borrow_mut()
                .push(format!("{program} {}", args.join(" ")));
            if program == "curl" {
                Ok(output(
                    "https://github.com/Plasticine-Yang/traex-session-manager/releases/tag/v0.2.0",
                ))
            } else {
                Ok(output("installed tsm 0.2.0\n"))
            }
        })
        .unwrap();

        assert_eq!(calls.borrow().len(), 2);
        assert!(calls.borrow()[1].contains("raw.githubusercontent.com"));
        assert!(calls.borrow()[1].contains("install.sh"));
        assert!(calls.borrow()[1].contains("-o \"$script\""));
        assert!(calls.borrow()[1].contains("sh \"$script\""));
    }

    #[test]
    fn version_comparison_is_numeric() {
        assert_eq!(
            compare_versions("0.10.0", "0.9.0").unwrap(),
            std::cmp::Ordering::Greater
        );
    }
}
