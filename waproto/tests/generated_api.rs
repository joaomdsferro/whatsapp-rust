//! Compile the *same external consumer* against both generations. The fixtures
//! are emitted in build.rs, using the resolved production generator dependency,
//! and use the production buffa dependency requirement and workspace lock.

use std::path::PathBuf;
use std::process::Command;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn external_consumer_survives_schema_evolution() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch =
        Scratch(std::env::temp_dir().join(format!("waproto-api-evolution-{}", std::process::id())));
    std::fs::create_dir_all(scratch.0.join("src")).unwrap();
    std::fs::create_dir_all(scratch.0.join("tests")).unwrap();
    // A separate crate is essential: non_exhaustive does not restrict literals
    // or matching within the crate defining the generated types.
    // Resolve the public runtime dependency without rebuilding the complete
    // WhatsApp schema in the small synthetic crate, twice for test/check.
    let metadata = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--locked",
            "--format-version=1",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "waproto")
        .unwrap();
    let runtime = package["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "buffa")
        .unwrap();
    let lock = PathBuf::from(metadata["workspace_root"].as_str().unwrap()).join("Cargo.lock");
    std::fs::copy(lock, scratch.0.join("Cargo.lock")).unwrap();
    let manifest = format!(
        r#"[package]
name = "evolution-fixture"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
buffa = {{ version = {:?}, default-features = {}, features = {} }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#,
        runtime["req"].as_str().unwrap(),
        runtime["uses_default_features"],
        runtime["features"]
    );
    std::fs::write(scratch.0.join("Cargo.toml"), manifest).unwrap();
    let generated = PathBuf::from(env!("OUT_DIR")).join("evolution");
    let source = format!(
        r#"#![allow(non_camel_case_types, unreachable_patterns)]
pub use buffa::*;
pub mod v1 {{ include!({:?}); }}
pub mod v2 {{ include!({:?}); }}
"#,
        generated.join("v1/contract.mod.rs"),
        generated.join("v2/contract.mod.rs")
    );
    std::fs::write(scratch.0.join("src/lib.rs"), source).unwrap();
    std::fs::write(
        scratch.0.join("tests/consumer.rs"),
        include_str!("fixtures/evolution_consumer.rs"),
    )
    .unwrap();
    let target = root.join("../target/generated-api-consumer");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO"))
            .args(args)
            .arg("--manifest-path")
            .arg(scratch.0.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target)
            .output()
            .expect("run external Cargo consumer")
    };
    let output = run(&["test", "--test", "consumer"]);
    assert!(
        output.status.success(),
        "external consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::write(
        scratch.0.join("tests/consumer.rs"),
        r#"
use evolution_fixture::v1::{Record, RecordView, record::{Mode, Choice}};
fn owned_literal() { let _ = Record { ..Default::default() }; }
fn view_literal() { let _ = RecordView { ..Default::default() }; }
fn enum_match(v: Mode) { match v { Mode::READY => {} } }
fn oneof_match(v: Choice) { match v { Choice::Text(_) => {} } }
fn view_oneof_match(v: evolution_fixture::v1::record::ChoiceView<'_>) {
    match v { evolution_fixture::v1::record::ChoiceView::Text(_) => {} }
}
"#,
    )
    .unwrap();
    let output = run(&["check", "--test", "consumer", "--message-format=json"]);
    assert!(
        !output.status.success(),
        "forbidden construction/matching unexpectedly compiled"
    );
    let diagnostics = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        diagnostics.matches("\"code\":\"E0639\"").count(),
        2,
        "{diagnostics}"
    );
    assert_eq!(
        diagnostics.matches("\"code\":\"E0004\"").count(),
        3,
        "{diagnostics}"
    );
}

#[test]
fn frozen_api_snapshot_includes_new_public_items() {
    let emitted = PathBuf::from(env!("OUT_DIR")).join("api.snapshot");
    let current = std::fs::read_to_string(&emitted).unwrap();
    assert!(
        current == include_str!("../api.snapshot"),
        "review and copy the emitted API inventory from {} to waproto/api.snapshot; additive entries also need protection before publishing",
        emitted.display()
    );
}
