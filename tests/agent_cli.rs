use std::process::Command;

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
