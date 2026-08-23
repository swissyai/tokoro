use super::report;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

pub(crate) const HANDOFF_SCHEMA: &str = "tokoro.handoff.v1";
const MANIFEST_FILE: &str = "HANDOFF.json";

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TargetInfo {
    pub id: &'static str,
    pub purpose: &'static str,
    pub files: &'static [&'static str],
    pub uploads: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct HandoffManifest {
    pub schema: String,
    pub target: String,
    pub bundle_sha256: String,
    pub captured_unix: u64,
    pub generator_version: String,
    pub files: Vec<HandoffFile>,
    pub custody: String,
    pub upload_performed: bool,
    pub verify: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct HandoffFile {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub media_type: String,
    pub purpose: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct HandoffPlan {
    pub schema: &'static str,
    pub target: String,
    pub bundle_sha256: String,
    pub files: Vec<String>,
    pub dry_run: bool,
    pub upload_performed: bool,
    pub verify: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct VerificationReceipt {
    pub schema: String,
    pub target: String,
    pub bundle_sha256: String,
    pub generator_version: String,
    pub files_verified: usize,
    pub upload_performed: bool,
    pub verdict: String,
}

struct PreparedFile {
    name: &'static str,
    content: Vec<u8>,
    media_type: &'static str,
    purpose: &'static str,
}

pub(crate) fn targets() -> Vec<TargetInfo> {
    vec![
        TargetInfo {
            id: "generic",
            purpose: "portable checked Markdown, JSON, and CSV",
            files: &["report.md", "bundle.json", "runs.csv", "HANDOFF.json"],
            uploads: false,
        },
        TargetInfo {
            id: "github",
            purpose: "GitHub issue or discussion body with checked attachments",
            files: &["issue-body.md", "bundle.json", "runs.csv", "HANDOFF.json"],
            uploads: false,
        },
        TargetInfo {
            id: "huggingface",
            purpose: "model-card section with checked result attachments",
            files: &[
                "model-card-section.md",
                "bundle.json",
                "runs.csv",
                "HANDOFF.json",
            ],
            uploads: false,
        },
        TargetInfo {
            id: "prometheus",
            purpose: "Prometheus text exposition from a checked report",
            files: &["tokoro.prom", "bundle.json", "HANDOFF.json"],
            uploads: false,
        },
        TargetInfo {
            id: "otlp",
            purpose: "OTLP JSON metrics from a checked report",
            files: &["tokoro-otlp.json", "bundle.json", "HANDOFF.json"],
            uploads: false,
        },
    ]
}

pub(crate) fn prepare(
    bundle_reference: &str,
    target: &str,
    output: &Path,
    replace: bool,
    dry_run: bool,
) -> Result<HandoffPlan, String> {
    let target = normalize_target(target)?;
    let envelope = report::load_measured(bundle_reference)?;
    let prepared = prepared_files(target, &envelope)?;
    let manifest = manifest(target, &envelope, &prepared);
    let plan = HandoffPlan {
        schema: HANDOFF_SCHEMA,
        target: target.into(),
        bundle_sha256: envelope.sha256.clone(),
        files: prepared
            .iter()
            .map(|file| file.name.to_string())
            .chain(std::iter::once(MANIFEST_FILE.into()))
            .collect(),
        dry_run,
        upload_performed: false,
        verify: "tokoro handoff verify OUTPUT_DIR --json".into(),
    };
    if dry_run {
        return Ok(plan);
    }
    write_atomic(output, replace, &manifest, &prepared)?;
    Ok(plan)
}

pub(crate) fn verify(directory: &Path) -> Result<VerificationReceipt, String> {
    let metadata = fs::symlink_metadata(directory).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("handoff path must be a real directory".into());
    }
    let manifest_path = directory.join(MANIFEST_FILE);
    let manifest_metadata =
        fs::symlink_metadata(&manifest_path).map_err(|error| error.to_string())?;
    if !manifest_metadata.is_file() || manifest_metadata.file_type().is_symlink() {
        return Err("HANDOFF.json must be a regular file".into());
    }
    let manifest = serde_json::from_str::<HandoffManifest>(
        &fs::read_to_string(&manifest_path).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("invalid handoff manifest: {error}"))?;
    if manifest.schema != HANDOFF_SCHEMA {
        return Err(format!("unsupported handoff schema '{}'", manifest.schema));
    }
    normalize_target(&manifest.target)?;
    if manifest.upload_performed {
        return Err("Tokoro handoffs must not claim an upload occurred".into());
    }
    if manifest.files.is_empty() {
        return Err("handoff manifest contains no artifacts".into());
    }
    let declared = manifest
        .files
        .iter()
        .map(|artifact| artifact.path.as_str())
        .collect::<HashSet<_>>();
    if declared.len() != manifest.files.len() {
        return Err("handoff manifest contains duplicate artifact paths".into());
    }
    let mut actual = HashSet::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry
            .file_name()
            .to_str()
            .map(str::to_string)
            .ok_or_else(|| "handoff contains a non-UTF-8 filename".to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!("handoff contains a non-file artifact '{name}'"));
        }
        actual.insert(name);
    }
    let expected = declared
        .iter()
        .map(|path| (*path).to_string())
        .chain(std::iter::once(MANIFEST_FILE.into()))
        .collect::<HashSet<_>>();
    if actual != expected {
        return Err("handoff directory contains missing or unlisted files".into());
    }
    for artifact in &manifest.files {
        validate_relative_file(&artifact.path)?;
        let path = directory.join(&artifact.path);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| format!("handoff artifact '{}' is missing", artifact.path))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!(
                "handoff artifact '{}' must be a regular file",
                artifact.path
            ));
        }
        let content = fs::read(&path).map_err(|error| error.to_string())?;
        if content.len() as u64 != artifact.bytes {
            return Err(format!("handoff artifact '{}' size changed", artifact.path));
        }
        if sha256(&content) != artifact.sha256 {
            return Err(format!(
                "handoff artifact '{}' SHA-256 changed",
                artifact.path
            ));
        }
    }
    let bundle_file = manifest
        .files
        .iter()
        .find(|artifact| artifact.path == "bundle.json")
        .ok_or_else(|| "handoff manifest does not include bundle.json".to_string())?;
    let bundle = serde_json::from_str::<report::ReportEnvelope>(
        &fs::read_to_string(directory.join(&bundle_file.path))
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("invalid checked bundle: {error}"))?;
    report::verify(&bundle)?;
    if bundle.sha256 != manifest.bundle_sha256 {
        return Err("handoff bundle receipt does not match its manifest".into());
    }
    Ok(VerificationReceipt {
        schema: manifest.schema,
        target: manifest.target,
        bundle_sha256: manifest.bundle_sha256,
        generator_version: manifest.generator_version,
        files_verified: manifest.files.len(),
        upload_performed: false,
        verdict: "verified".into(),
    })
}

fn prepared_files(
    target: &str,
    envelope: &report::ReportEnvelope,
) -> Result<Vec<PreparedFile>, String> {
    let bundle = report::render_json(envelope)?.into_bytes();
    let markdown = report::render_markdown(envelope, &report::ReportRecipe::default())?;
    let csv = report::render_csv(envelope)?.into_bytes();
    let files = match target {
        "generic" => vec![
            prepared(
                "report.md",
                markdown,
                "text/markdown",
                "readable checked report",
            ),
            prepared(
                "bundle.json",
                bundle,
                "application/json",
                "immutable measurement bundle",
            ),
            prepared("runs.csv", csv, "text/csv", "portable measurement rows"),
        ],
        "github" => vec![
            prepared(
                "issue-body.md",
                github_markdown(envelope, &markdown),
                "text/markdown",
                "paste-ready GitHub issue or discussion body",
            ),
            prepared(
                "bundle.json",
                bundle,
                "application/json",
                "checked attachment",
            ),
            prepared("runs.csv", csv, "text/csv", "measurement attachment"),
        ],
        "huggingface" => vec![
            prepared(
                "model-card-section.md",
                huggingface_markdown(envelope, &markdown),
                "text/markdown",
                "paste-ready model-card section",
            ),
            prepared(
                "bundle.json",
                bundle,
                "application/json",
                "checked result attachment",
            ),
            prepared("runs.csv", csv, "text/csv", "measurement attachment"),
        ],
        "prometheus" => vec![
            prepared(
                "tokoro.prom",
                report::render_prometheus(envelope)?,
                "text/plain",
                "Prometheus text exposition",
            ),
            prepared(
                "bundle.json",
                bundle,
                "application/json",
                "checked source bundle",
            ),
        ],
        "otlp" => vec![
            prepared(
                "tokoro-otlp.json",
                report::render_otlp_json(envelope)?,
                "application/json",
                "OTLP JSON metrics payload",
            ),
            prepared(
                "bundle.json",
                bundle,
                "application/json",
                "checked source bundle",
            ),
        ],
        _ => return Err("unsupported handoff target".into()),
    };
    Ok(files)
}

fn prepared(
    name: &'static str,
    content: impl Into<Vec<u8>>,
    media_type: &'static str,
    purpose: &'static str,
) -> PreparedFile {
    PreparedFile {
        name,
        content: content.into(),
        media_type,
        purpose,
    }
}

fn manifest(
    target: &str,
    envelope: &report::ReportEnvelope,
    files: &[PreparedFile],
) -> HandoffManifest {
    HandoffManifest {
        schema: HANDOFF_SCHEMA.into(),
        target: target.into(),
        bundle_sha256: envelope.sha256.clone(),
        captured_unix: envelope.data.captured_unix,
        generator_version: env!("CARGO_PKG_VERSION").into(),
        files: files
            .iter()
            .map(|file| HandoffFile {
                path: file.name.into(),
                sha256: sha256(&file.content),
                bytes: file.content.len() as u64,
                media_type: file.media_type.into(),
                purpose: file.purpose.into(),
            })
            .collect(),
        custody: "local files; explicit user handoff required".into(),
        upload_performed: false,
        verify: "tokoro handoff verify OUTPUT_DIR --json".into(),
    }
}

fn write_atomic(
    output: &Path,
    replace: bool,
    manifest: &HandoffManifest,
    files: &[PreparedFile],
) -> Result<(), String> {
    if output.exists() {
        if let Ok(existing) = verify(output) {
            if existing.bundle_sha256 == manifest.bundle_sha256
                && existing.target == manifest.target
                && existing.generator_version == env!("CARGO_PKG_VERSION")
            {
                return Ok(());
            }
            if !replace {
                return Err(
                    "output already contains a different verified Tokoro handoff; add --replace"
                        .into(),
                );
            }
        } else {
            return Err(
                "refusing to replace a directory that is not a verified Tokoro handoff".into(),
            );
        }
        fs::remove_dir_all(output).map_err(|error| error.to_string())?;
    }
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let name = output
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "handoff output needs a directory name".to_string())?;
    let staging = parent.join(format!(".{name}.tokoro-{}.staging", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| error.to_string())?;
    }
    fs::create_dir(&staging).map_err(|error| error.to_string())?;
    let result = (|| {
        for file in files {
            fs::write(staging.join(file.name), &file.content).map_err(|error| error.to_string())?;
        }
        fs::write(
            staging.join(MANIFEST_FILE),
            serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::rename(&staging, output).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn github_markdown(envelope: &report::ReportEnvelope, markdown: &str) -> String {
    format!(
        "<!-- Tokoro checked bundle {} -->\n\n{}\n\nAttach `bundle.json` and `runs.csv`. Verify locally with `tokoro handoff verify OUTPUT_DIR`.\n",
        &envelope.sha256[..12], markdown
    )
}

fn huggingface_markdown(envelope: &report::ReportEnvelope, markdown: &str) -> String {
    let demoted = markdown
        .lines()
        .map(|line| {
            if line.starts_with('#') {
                format!("#{line}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "## Local inference measurement\n\nChecked with `tokoro.report.v1` bundle `{}`. This is a machine-speed result, not a quality endorsement.\n\n{}\n",
        &envelope.sha256[..12], demoted
    )
}

fn normalize_target(value: &str) -> Result<&'static str, String> {
    match value.to_ascii_lowercase().as_str() {
        "generic" | "files" => Ok("generic"),
        "github" | "github-issue" => Ok("github"),
        "huggingface" | "hf" | "model-card" => Ok("huggingface"),
        "prometheus" | "prom" => Ok("prometheus"),
        "otlp" | "opentelemetry" | "otlp-json" => Ok("otlp"),
        _ => Err("handoff target must be generic, github, huggingface, prometheus, or otlp".into()),
    }
}

fn validate_relative_file(value: &str) -> Result<(), String> {
    let path = PathBuf::from(value);
    let mut components = path.components();
    if value.is_empty()
        || value == MANIFEST_FILE
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(format!("invalid handoff artifact path '{value}'"));
    }
    Ok(())
}

fn sha256(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{App, BenchRun, Config};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn checked_bundle(path: &Path) {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        app.chip = "Test Chip".into();
        app.total_mem_gb = 64.0;
        app.model = "test-model".into();
        app.engine = "test-runtime".into();
        app.bench.started_unix = 1_700_000_000;
        app.bench.label = "quick response".into();
        app.bench.prompt_tokens = 512;
        app.bench.gen_tokens = 64;
        app.bench.runs = 1;
        app.bench.results.push(BenchRun {
            pp: 100.0,
            tg: 20.0,
            ttft_ms: 80.0,
            tpot_ms: 47.3,
            end_to_end_ms: 600.0,
            output_tokens: 12,
            token_count_source: "server-reported usage".into(),
        });
        let envelope = report::capture(&app).expect("capture");
        fs::write(path, report::render_json(&envelope).expect("JSON")).expect("write bundle");
    }

    fn temp_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("tokoro-handoff-{label}-{nonce}"))
    }

    #[test]
    fn prepares_and_verifies_a_checked_handoff_without_uploading() {
        let root = temp_root("verified");
        fs::create_dir_all(&root).expect("temp root");
        let bundle = root.join("source.json");
        let output = root.join("github");
        checked_bundle(&bundle);
        let plan = prepare(
            bundle.to_str().expect("bundle path"),
            "github",
            &output,
            false,
            false,
        )
        .expect("prepare");
        assert!(!plan.upload_performed);
        assert!(output.join("issue-body.md").is_file());
        let receipt = verify(&output).expect("verify");
        assert_eq!(receipt.verdict, "verified");
        assert_eq!(receipt.files_verified, 3);
        let manifest = fs::read_to_string(output.join(MANIFEST_FILE)).expect("manifest");
        assert!(!manifest.contains("/private/"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn dry_run_is_side_effect_free_and_tampering_fails_verification() {
        let root = temp_root("dry-run");
        fs::create_dir_all(&root).expect("temp root");
        let bundle = root.join("source.json");
        let output = root.join("generic");
        checked_bundle(&bundle);
        let plan = prepare(
            bundle.to_str().expect("bundle path"),
            "generic",
            &output,
            false,
            true,
        )
        .expect("dry run");
        assert!(plan.dry_run);
        assert!(!output.exists());
        prepare(
            bundle.to_str().expect("bundle path"),
            "generic",
            &output,
            false,
            false,
        )
        .expect("prepare");
        fs::write(output.join("unlisted.txt"), "not declared").expect("extra file");
        assert!(verify(&output).is_err());
        fs::remove_file(output.join("unlisted.txt")).expect("remove extra file");
        fs::write(output.join("report.md"), "tampered").expect("tamper");
        assert!(verify(&output).is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn manifest_rejects_parent_traversal_and_manifest_self_reference() {
        assert!(validate_relative_file("../bundle.json").is_err());
        assert!(validate_relative_file("nested/bundle.json").is_err());
        assert!(validate_relative_file(MANIFEST_FILE).is_err());
        assert!(validate_relative_file("bundle.json").is_ok());
    }
}
