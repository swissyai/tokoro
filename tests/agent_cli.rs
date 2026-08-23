use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn run(args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_tokoro"))
        .args(args)
        .env("CI", "true")
        .output()
        .expect("run Tokoro CLI");
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("machine-readable JSON")
}

#[test]
fn stable_agent_commands_are_machine_readable() {
    let payload = run(&["commands", "--json"]);
    assert_eq!(payload["schema"], "tokoro.agent.v1");
    assert_eq!(payload["kind"], "command_catalog");
    assert!(payload["commands"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
}

#[test]
fn monitoring_posture_is_typed_and_names_current_cues() {
    let payload = run(&["monitor", "--json"]);
    assert_eq!(payload["schema"], "tokoro.agent.v1");
    assert_eq!(payload["kind"], "monitoring_posture");
    assert_eq!(payload["monitoring_schema"], "tokoro.monitoring.v1");
    assert!(payload["summary"]["score"].is_null());
    assert!(payload["layers"]
        .as_array()
        .is_some_and(|layers| layers.iter().any(|layer| layer["id"] == "scheduler_cache")));
    assert!(payload["cues"]
        .as_array()
        .is_some_and(|cues| !cues.is_empty()));
}

#[test]
fn visualization_profiles_are_typed_palette_independent_data() {
    let profiles = run(&["visualization", "list", "--json"]);
    assert_eq!(profiles["kind"], "visualization_profiles");
    assert_eq!(profiles["profile_schema"], "tokoro.visualization.v1");
    let builtins = profiles["profiles"].as_array().expect("profile array");
    for name in ["tokoro", "operator", "focus", "mono"] {
        assert!(builtins.iter().any(|profile| profile["name"] == name));
    }
    assert!(builtins
        .iter()
        .filter(|profile| profile["name"].is_string())
        .all(|profile| profile["palette"] == "separate"));

    let schema = run(&["visualization", "schema", "--json"]);
    assert_eq!(schema["kind"], "visualization_schema");
    assert_eq!(schema["json_schema"]["additionalProperties"], false);
    assert_eq!(
        schema["json_schema"]["boundaries"]["code"],
        "data-only TOML; executable plugins are not loaded"
    );
}

#[test]
fn custom_visualization_requires_a_checked_local_application() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("tokoro-visualization-test-{nonce}"));
    fs::create_dir_all(&root).expect("test directory");
    let source = root.join("quiet.toml");
    fs::write(
        &source,
        r#"schema = "tokoro.visualization.v1"
name = "quiet-test"
description = "Integration-test profile with a portable renderer."
density = "compact"
layout = "stacked"
graph_renderer = "ascii"
history_window = 40
panel_order = ["model", "next", "capacity", "inventory", "performance", "stages", "streams", "history", "memory", "sources", "interference", "bloat"]
"#,
    )
    .expect("profile fixture");

    let refused = Command::new(env!("CARGO_BIN_EXE_tokoro"))
        .args(["visualization", "apply"])
        .arg(&source)
        .arg("--json")
        .env("CI", "true")
        .env("XDG_CONFIG_HOME", &root)
        .output()
        .expect("refuse unconfirmed profile");
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("rerun with --confirm"));
    assert!(!root.join("tokoro/visualizations/quiet-test.toml").exists());

    let output = Command::new(env!("CARGO_BIN_EXE_tokoro"))
        .args(["visualization", "apply"])
        .arg(&source)
        .args(["--confirm", "--json"])
        .env("CI", "true")
        .env("XDG_CONFIG_HOME", &root)
        .output()
        .expect("apply profile");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("application JSON");
    assert_eq!(payload["applied"], true);
    assert_eq!(payload["palette_change"], false);
    assert!(root.join("tokoro/visualizations/quiet-test.toml").is_file());

    let current = Command::new(env!("CARGO_BIN_EXE_tokoro"))
        .args(["visualization", "current", "--json"])
        .env("CI", "true")
        .env("XDG_CONFIG_HOME", &root)
        .output()
        .expect("read current profile");
    let current: serde_json::Value = serde_json::from_slice(&current.stdout).expect("current JSON");
    assert_eq!(current["state"], "valid");
    assert_eq!(current["profile"]["name"], "quiet-test");
    assert_eq!(current["profile"]["graph_renderer"], "ascii");

    fs::remove_dir_all(root).expect("clean test directory");
}

#[test]
fn integration_and_handoff_catalogs_are_noninteractive() {
    let integrations = run(&["integrations", "--json"]);
    assert_eq!(integrations["kind"], "integration_catalog");
    assert_eq!(integrations["policy"]["uploads"], "never_automatic");
    assert!(integrations["report_handoffs"]
        .as_array()
        .is_some_and(|targets| targets.iter().any(|target| target["id"] == "github")));

    let handoffs = run(&["handoff", "list", "--json"]);
    assert_eq!(handoffs["kind"], "handoff_targets");
    assert!(handoffs["targets"]
        .as_array()
        .is_some_and(|targets| targets.iter().all(|target| target["uploads"] == false)));
}
