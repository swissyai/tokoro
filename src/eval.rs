use super::platform;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::SystemTime};

const EVAL_SCHEMA: &str = "tokoro.eval.v1";
const MAX_CONTENT_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct EvalSeed {
    pub label: String,
    pub request_id: String,
    pub model: String,
    pub engine: String,
    pub prompt_tokens: u32,
    pub output_tokens: u32,
    pub ttft_milliseconds: Option<f64>,
    pub decode_tokens_per_second: f64,
    pub sampling: String,
    pub outcome: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EvalEnvelope {
    schema: String,
    sha256: String,
    data: EvalData,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EvalData {
    captured_unix: u64,
    label: String,
    source: SourceMeasurement,
    content: ContentReceipt,
    custody: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct SourceMeasurement {
    kind: String,
    request_id: String,
    model: String,
    engine: String,
    prompt_tokens: u32,
    output_tokens: u32,
    ttft_milliseconds: Option<f64>,
    decode_tokens_per_second: f64,
    sampling: String,
    outcome: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ContentReceipt {
    prompt_in_private_file: bool,
    expected_in_private_file: bool,
    prompt_sha256: Option<String>,
    expected_sha256: Option<String>,
    bodies_in_fixture_json: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Review {
    status: String,
    note: String,
    judge: String,
}

impl Default for Review {
    fn default() -> Self {
        Self {
            status: "unreviewed".into(),
            note: String::new(),
            judge: "human".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct EvalListEntry {
    pub id: String,
    pub label: String,
    pub status: String,
    pub has_prompt: bool,
    pub has_expected: bool,
    pub source: String,
}

pub(crate) fn create_from_measurement(seed: EvalSeed) -> Result<String, String> {
    let data = EvalData {
        captured_unix: unix_now(),
        label: clean_label(&seed.label)?,
        source: SourceMeasurement {
            kind: "selected_request_metrics".into(),
            request_id: clean_token(&seed.request_id),
            model: clean_token(&seed.model),
            engine: clean_token(&seed.engine),
            prompt_tokens: seed.prompt_tokens,
            output_tokens: seed.output_tokens,
            ttft_milliseconds: seed.ttft_milliseconds,
            decode_tokens_per_second: seed.decode_tokens_per_second,
            sampling: clean_note(&seed.sampling),
            outcome: clean_token(&seed.outcome),
        },
        content: ContentReceipt::default(),
        custody: "local_private_metrics_only".into(),
    };
    save_fixture(data, None, None)
}

pub(crate) fn create_manual(
    label: &str,
    prompt_path: Option<&Path>,
    expected_path: Option<&Path>,
) -> Result<String, String> {
    let prompt = prompt_path.map(read_explicit_content).transpose()?;
    let expected = expected_path.map(read_explicit_content).transpose()?;
    let data = EvalData {
        captured_unix: unix_now(),
        label: clean_label(label)?,
        source: SourceMeasurement {
            kind: "manual_fixture".into(),
            outcome: "unreviewed".into(),
            ..Default::default()
        },
        content: ContentReceipt {
            prompt_in_private_file: prompt.is_some(),
            expected_in_private_file: expected.is_some(),
            prompt_sha256: prompt.as_deref().map(content_hash),
            expected_sha256: expected.as_deref().map(content_hash),
            bodies_in_fixture_json: false,
        },
        custody: "local_private_explicit_content".into(),
    };
    save_fixture(data, prompt.as_deref(), expected.as_deref())
}

pub(crate) fn list() -> Result<Vec<EvalListEntry>, String> {
    let root = eval_root();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry.path().is_dir() {
            continue;
        }
        let Ok(envelope) = load_fixture(&entry.path()) else {
            continue;
        };
        if verify(&envelope).is_err() {
            continue;
        }
        let review = load_review(&entry.path()).unwrap_or_default();
        entries.push(EvalListEntry {
            id: envelope.sha256[..12].into(),
            label: envelope.data.label,
            status: review.status,
            has_prompt: envelope.data.content.prompt_in_private_file,
            has_expected: envelope.data.content.expected_in_private_file,
            source: envelope.data.source.kind,
        });
    }
    entries.sort_by(|left, right| left.label.cmp(&right.label).then(left.id.cmp(&right.id)));
    Ok(entries)
}

pub(crate) fn review(id: &str, status: &str, note: &str) -> Result<(), String> {
    if !matches!(status, "pass" | "fail") {
        return Err("eval review status must be pass or fail".into());
    }
    let directory = fixture_directory(id)?;
    let envelope = load_fixture(&directory)?;
    verify(&envelope)?;
    let review = Review {
        status: status.into(),
        note: clean_note(note),
        judge: "human".into(),
    };
    fs::write(
        directory.join("review.toml"),
        toml::to_string_pretty(&review).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn show(id: &str) -> Result<serde_json::Value, String> {
    let directory = fixture_directory(id)?;
    let envelope = load_fixture(&directory)?;
    verify(&envelope)?;
    let review = load_review(&directory).unwrap_or_default();
    Ok(serde_json::json!({
        "schema": EVAL_SCHEMA,
        "id": &envelope.sha256[..12],
        "fixture": envelope.data,
        "review": review,
        "content_bodies_included": false,
        "custody": "local_private",
    }))
}

fn save_fixture(
    data: EvalData,
    prompt: Option<&str>,
    expected: Option<&str>,
) -> Result<String, String> {
    let canonical = serde_json::to_vec(&data).map_err(|error| error.to_string())?;
    let sha256 = format!("{:x}", Sha256::digest(canonical));
    let envelope = EvalEnvelope {
        schema: EVAL_SCHEMA.into(),
        sha256: sha256.clone(),
        data,
    };
    let id = sha256[..12].to_string();
    let root = eval_root();
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let directory = root.join(&id);
    if directory.exists() {
        return Ok(id);
    }
    let staging = root.join(format!(".{id}.staging"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| error.to_string())?;
    }
    fs::create_dir(&staging).map_err(|error| error.to_string())?;
    fs::write(
        staging.join("fixture.json"),
        serde_json::to_string_pretty(&envelope).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        staging.join("review.toml"),
        toml::to_string_pretty(&Review::default()).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if let Some(prompt) = prompt {
        fs::write(staging.join("prompt.txt"), prompt).map_err(|error| error.to_string())?;
    }
    if let Some(expected) = expected {
        fs::write(staging.join("expected.txt"), expected).map_err(|error| error.to_string())?;
    }
    fs::rename(&staging, &directory).map_err(|error| error.to_string())?;
    Ok(id)
}

fn read_explicit_content(path: &Path) -> Result<String, String> {
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();
    if filename == ".env"
        || filename.contains("credential")
        || filename.contains("secret")
        || filename.contains("private_key")
        || filename.ends_with(".pem")
    {
        return Err("refusing credential-shaped eval content".into());
    }
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_CONTENT_BYTES {
        return Err("eval content must be a regular text file no larger than 1 MiB".into());
    }
    fs::read_to_string(path).map_err(|error| error.to_string())
}

fn verify(envelope: &EvalEnvelope) -> Result<(), String> {
    if envelope.schema != EVAL_SCHEMA {
        return Err(format!("unsupported eval schema '{}'", envelope.schema));
    }
    let canonical = serde_json::to_vec(&envelope.data).map_err(|error| error.to_string())?;
    let expected = format!("{:x}", Sha256::digest(canonical));
    if expected != envelope.sha256 {
        return Err("eval fixture SHA-256 does not match its metadata".into());
    }
    Ok(())
}

fn fixture_directory(id: &str) -> Result<std::path::PathBuf, String> {
    if id.len() != 12 || !id.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("eval id must be a 12-character hexadecimal receipt".into());
    }
    let directory = eval_root().join(id);
    if !directory.is_dir() {
        return Err(format!("eval fixture '{id}' was not found"));
    }
    Ok(directory)
}

fn load_fixture(directory: &Path) -> Result<EvalEnvelope, String> {
    serde_json::from_str(
        &fs::read_to_string(directory.join("fixture.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn load_review(directory: &Path) -> Result<Review, String> {
    toml::from_str(
        &fs::read_to_string(directory.join("review.toml")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn eval_root() -> std::path::PathBuf {
    platform::state_home().join("tokoro").join("evals")
}

fn content_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn clean_label(value: &str) -> Result<String, String> {
    let value = clean_note(value);
    if value.is_empty() {
        return Err("eval label must not be empty".into());
    }
    Ok(value.chars().take(120).collect())
}

fn clean_note(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(500)
        .collect()
}

fn clean_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
        .take(160)
        .collect()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_json_never_contains_explicit_content_bodies() {
        let data = EvalData {
            captured_unix: 1,
            label: "private test".into(),
            source: SourceMeasurement::default(),
            content: ContentReceipt {
                prompt_in_private_file: true,
                expected_in_private_file: true,
                prompt_sha256: Some(content_hash("private prompt")),
                expected_sha256: Some(content_hash("private expected")),
                bodies_in_fixture_json: false,
            },
            custody: "local_private_explicit_content".into(),
        };
        let json = serde_json::to_string(&data).expect("serialize");
        assert!(!json.contains("private prompt"));
        assert!(!json.contains("private expected"));
        assert!(json.contains("prompt_sha256"));
    }

    #[test]
    fn credential_shaped_content_is_refused_before_reading() {
        assert!(read_explicit_content(Path::new(".env")).is_err());
        assert!(read_explicit_content(Path::new("private_key.pem")).is_err());
    }
}
