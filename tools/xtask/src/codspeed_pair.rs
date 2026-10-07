//! Contracts for the fixed, upload-disabled A02 benchmark comparison.
use anyhow::{Context, Result, ensure};
use clap::{Subcommand, ValueEnum};
use std::collections::BTreeMap;
use std::fs::write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use xtask_support::capture;

const BASE: &str = "bac94e7c3c83fb7a283f3b551b233c476b283cfa";
const HEAD: &str = "8d184150bec8c65ef9aca9d84349dc831710f566";
const CONTRACTS: &[&str] = &[
    "runtime.sha256",
    "cpu-contract.txt",
    "rustc.txt",
    "cargo-codspeed.txt",
    "build-env.txt",
];
const ENVIRONMENT: &[&str] = &[
    "MALLOC_ARENA_MAX",
    "MALLOC_MMAP_THRESHOLD_",
    "MALLOC_TRIM_THRESHOLD_",
    "MALLOC_TOP_PAD_",
    "GLIBC_TUNABLES",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "CARGO_INCREMENTAL",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
];
const REQUIRED: &[&str] = &[
    "bench_collect_unique_index_macs_duplicates",
    "store_round_trip_rebuilt",
    "bench_sender_key_serialize_without_backlog",
    "bench_session_serialize_with_backlog",
    "bench_reject_prekey_as_signal",
    "plaintext_ownership_controls",
];

#[derive(Clone, Copy, ValueEnum)]
pub enum Side {
    Base,
    Head,
}
impl Side {
    fn name(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Head => "head",
        }
    }
}
#[derive(Subcommand)]
pub enum Task {
    Preflight,
    Capture {
        #[arg(long, value_enum)]
        side: Side,
    },
    Validate,
}

fn env_path(key: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os(key).with_context(|| format!("missing {key}"))?,
    ))
}
fn output(program: &str, args: &[&str]) -> Result<Vec<u8>> {
    Ok(capture(Command::new(program).args(args))?.stdout)
}
fn version(result: &Output) -> Result<&'static str> {
    // 5.0.1 propagates clap's DisplayVersion through anyhow, then exits 1.
    ensure!(
        result.status.code() == Some(1)
            && result.stdout.is_empty()
            && std::str::from_utf8(&result.stderr)?.trim() == "cargo-codspeed 5.0.1",
        "unexpected cargo-codspeed version response: {result:?}"
    );
    Ok("cargo-codspeed 5.0.1\n")
}
fn contracts(out: &Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    write(
        out.join("runtime.sha256"),
        output(
            "sha256sum",
            &[
                "/lib/x86_64-linux-gnu/libc.so.6",
                "/lib/x86_64-linux-gnu/libm.so.6",
            ],
        )?,
    )?;
    let cpu = std::fs::read_to_string("/proc/cpuinfo")?;
    let mut lines: Vec<_> = cpu
        .lines()
        .filter(|line| {
            ["vendor_id", "model name", "flags"]
                .iter()
                .any(|prefix| line.starts_with(prefix))
        })
        .collect();
    lines.sort_unstable();
    lines.dedup();
    ensure!(!lines.is_empty(), "missing CPU contract");
    write(
        out.join("cpu-contract.txt"),
        format!("{}\n", lines.join("\n")),
    )?;
    write(out.join("rustc.txt"), output("rustc", &["-Vv"])?)?;
    write(
        out.join("cargo-codspeed.txt"),
        version(
            &Command::new("cargo")
                .args(["codspeed", "--version"])
                .output()?,
        )?,
    )?;
    let env: BTreeMap<_, _> = ENVIRONMENT
        .iter()
        .map(|key| {
            (
                *key,
                std::env::var_os(key).map(|value| format!("{value:?}")),
            )
        })
        .collect();
    write(out.join("build-env.txt"), serde_json::to_vec_pretty(&env)?)?;
    Ok(())
}
fn equal(left: &Path, right: &Path, name: &str) -> Result<()> {
    ensure!(
        std::fs::read(left.join(name))? == std::fs::read(right.join(name))?,
        "contract differs: {name}"
    );
    Ok(())
}
fn profile(directory: &Path) -> Result<PathBuf> {
    let profiles: Vec<_> = std::fs::read_dir(directory)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with("profile.") && name.ends_with(".out")
                })
        })
        .collect();
    ensure!(
        profiles.len() == 1,
        "expected one profile under {}, got {}",
        directory.display(),
        profiles.len()
    );
    Ok(profiles[0].clone())
}
fn measured(profile: &Path) -> Result<Vec<String>> {
    let log = std::fs::read_to_string(profile.join("runner.log"))?;
    let mut names: Vec<_> = log
        .lines()
        .filter_map(|line| {
            line.split_once("Measured: ")
                .map(|(_, name)| name.to_owned())
        })
        .collect();
    names.sort();
    ensure!(!names.is_empty(), "no measured benchmarks");
    for required in REQUIRED {
        ensure!(
            names.iter().any(|name| name.contains(required)),
            "missing measured benchmark: {required}"
        );
    }
    let mut has_events = false;
    for entry in std::fs::read_dir(profile)? {
        let path = entry?.path();
        if !path.is_file()
            || !path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with(|c: char| c.is_ascii_digit()) && name.ends_with(".out")
            })
        {
            continue;
        }
        let text = std::fs::read_to_string(path)?;
        has_events |= text.lines().any(|line| {
            line.strip_prefix("events: ")
                .is_some_and(|fields| !fields.trim().is_empty())
        }) && text.lines().any(|line| {
            line.strip_prefix("summary:").is_some_and(|values| {
                values
                    .split_whitespace()
                    .any(|value| value.parse::<u64>().is_ok_and(|value| value > 0))
            })
        });
    }
    ensure!(has_events, "no nonempty instrumented profile");
    Ok(names)
}
fn validate(root: &Path) -> Result<()> {
    let base = profile(&root.join("base"))?;
    let head = profile(&root.join("head"))?;
    for (path, sha) in [(&base, BASE), (&head, HEAD)] {
        ensure!(
            std::fs::read_to_string(path.join("source-commit.txt"))?.trim() == sha,
            "unexpected measured source"
        );
    }
    let base_names = measured(&base)?;
    let head_names = measured(&head)?;
    ensure!(base_names == head_names, "measured benchmark sets differ");
    for side in ["base", "head"] {
        write(
            root.join(side).join("measured.txt"),
            format!("{}\n", base_names.join("\n")),
        )?;
    }
    for name in CONTRACTS.iter().copied().chain(["runner-version.txt"]) {
        equal(&base, &head, name)?;
    }
    Ok(())
}
pub fn run(task: Task) -> Result<()> {
    let workspace = env_path("GITHUB_WORKSPACE")?.canonicalize()?;
    let profiles = workspace.join("_a02_profiles");
    match task {
        Task::Preflight => {
            for program in ["cc", "c++", "cmake", "make", "perl", "pkg-config"] {
                capture(Command::new(program).arg("--version"))?;
            }
            let mut child = Command::new("cc")
                .args(["-x", "c", "-fsyntax-only", "-"])
                .stdin(Stdio::piped())
                .spawn()?;
            {
                use std::io::Write;
                child
                    .stdin
                    .take()
                    .context("compiler stdin")?
                    .write_all(b"#include <stdlib.h>\n#include <pthread.h>\n")?;
            }
            ensure!(child.wait()?.success(), "native headers unavailable");
            for side in ["base", "head", "preflight"] {
                std::fs::create_dir_all(profiles.join(side))?;
            }
            ensure!(
                profiles.join("base").canonicalize()? != profiles.join("head").canonicalize()?,
                "profile directories alias"
            );
            contracts(&profiles.join("preflight"))?;
        }
        Task::Capture { side } => {
            ensure!(
                std::env::var("CODSPEED_SKIP_UPLOAD").as_deref() == Ok("true"),
                "uploads must remain disabled"
            );
            let destination = env_path("CODSPEED_PROFILE_FOLDER")?.canonicalize()?;
            let temp = env_path("TMPDIR")?.canonicalize()?;
            ensure!(
                destination.parent() == Some(temp.as_path())
                    && temp == profiles.join(side.name()).canonicalize()?,
                "profile escaped its side directory"
            );
            ensure!(
                std::env::current_dir()?.canonicalize()?
                    == workspace
                        .join("_a02_pair")
                        .join(side.name())
                        .canonicalize()?,
                "wrong benchmark checkout"
            );
            contracts(&destination)?;
            write(
                destination.join("source-commit.txt"),
                output("git", &["rev-parse", "HEAD"])?,
            )?;
            write(
                destination.join("lock.sha256"),
                output("sha256sum", &["Cargo.lock"])?,
            )?;
            write(
                destination.join("runner-version.txt"),
                output("codspeed", &["--version"])?,
            )?;
            write(
                destination.join("cpuinfo.txt"),
                std::fs::read("/proc/cpuinfo")?,
            )?;
            for name in CONTRACTS {
                equal(&profiles.join("preflight"), &destination, name)?;
            }
            if matches!(side, Side::Head) {
                equal(
                    &profile(&profiles.join("base"))?,
                    &destination,
                    "runner-version.txt",
                )?;
            }
        }
        Task::Validate => validate(&profiles)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn version_accepts_only_the_pinned_display_version_failure() {
        use std::os::unix::process::ExitStatusExt;
        let mut result = Output {
            status: std::process::ExitStatus::from_raw(256),
            stdout: vec![],
            stderr: b"cargo-codspeed 5.0.1\n\n".to_vec(),
        };
        assert!(version(&result).is_ok());
        result.stderr = b"cargo-codspeed 0.0.0\n".to_vec();
        assert!(version(&result).is_err());
        result.stderr = b"cargo-codspeed 5.0.1\n".to_vec();
        result.status = std::process::ExitStatus::from_raw(127 << 8);
        assert!(version(&result).is_err());
    }
    #[test]
    fn pair_rejects_changed_contracts_missing_measurements_and_empty_profiles() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for (side, sha) in [("base", BASE), ("head", HEAD)] {
            let path = root.join(side).join("profile.1.out");
            std::fs::create_dir_all(&path).unwrap();
            write(path.join("source-commit.txt"), sha).unwrap();
            for name in CONTRACTS.iter().copied().chain(["runner-version.txt"]) {
                write(path.join(name), "same").unwrap();
            }
            write(
                path.join("runner.log"),
                REQUIRED
                    .iter()
                    .map(|name| format!("Measured: {name}\n"))
                    .collect::<String>(),
            )
            .unwrap();
            write(path.join("1.out"), "events: Ir\nsummary: 12\n").unwrap();
        }
        assert!(validate(root).is_ok());
        let head = root.join("head/profile.1.out");
        write(head.join("runtime.sha256"), "different").unwrap();
        assert!(validate(root).is_err());
        write(head.join("runtime.sha256"), "same").unwrap();
        write(head.join("1.out"), "events: Ir\nsummary: 0\n").unwrap();
        assert!(validate(root).is_err());
        write(head.join("1.out"), "events: Ir\nsummary: 12\n").unwrap();
        write(head.join("runner.log"), "Measured: unrelated\n").unwrap();
        assert!(validate(root).is_err());
        std::fs::create_dir(root.join("base/profile.2.out")).unwrap();
        assert!(profile(&root.join("base")).is_err());
    }
}
