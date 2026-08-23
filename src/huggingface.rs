use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Duration,
};

const API_ROOT: &str = "https://huggingface.co/api/models";
const DOWNLOAD_ROOT: &str = "https://huggingface.co";
const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;
const DISK_RESERVE_BYTES: u64 = BYTES_PER_GIB;

#[derive(Clone, Copy)]
pub struct Candidate {
    pub repo: &'static str,
    pub label: &'static str,
    pub purpose: &'static str,
}

pub const STARTERS: &[Candidate] = &[
    Candidate {
        repo: "HuggingFaceTB/SmolLM2-135M-Instruct",
        label: "SmolLM2 135M Instruct | safetensors",
        purpose: "portable 260 MiB artifact for Linux and custom runtimes",
    },
    Candidate {
        repo: "mlx-community/SmolLM2-135M-Instruct-8bit",
        label: "SmolLM2 135M Instruct | 8-bit",
        purpose: "tiny download and runtime smoke test",
    },
    Candidate {
        repo: "mlx-community/Qwen2.5-0.5B-Instruct-4bit",
        label: "Qwen2.5 0.5B Instruct | 4-bit",
        purpose: "small chat and tool-call starter",
    },
    Candidate {
        repo: "mlx-community/Qwen3-0.6B-4bit",
        label: "Qwen3 0.6B | 4-bit",
        purpose: "small Qwen runtime check",
    },
];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub repo: String,
    pub revision: String,
    pub total_bytes: u64,
    pub files: Vec<RemoteFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RemoteFile {
    pub name: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Clone)]
pub struct Entry {
    pub repo: String,
    pub label: String,
    pub purpose: String,
    manifest: Option<Manifest>,
    installed: bool,
    error: Option<String>,
}

impl Entry {
    pub fn state_label(&self) -> &'static str {
        if self.installed {
            "installed"
        } else if self.error.is_some() {
            "check failed"
        } else if self.manifest.is_some() {
            "manifest checked"
        } else {
            "not checked"
        }
    }

    pub fn detail(&self) -> String {
        if let Some(manifest) = &self.manifest {
            format!(
                "{} | {} | commit {}",
                format_bytes(manifest.total_bytes),
                self.state_label(),
                &manifest.revision[..8]
            )
        } else if let Some(error) = &self.error {
            error.clone()
        } else {
            self.purpose.clone()
        }
    }

    pub fn manifest(&self) -> Option<&Manifest> {
        self.manifest.as_ref()
    }

    pub const fn installed(&self) -> bool {
        self.installed
    }
}

pub enum Event {
    Checked(usize),
    Searched(usize),
    Downloaded { label: String, target: PathBuf },
    Failed(String),
}

#[derive(Clone, Copy, Default)]
pub struct Progress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

pub struct Catalog {
    entries: Vec<Entry>,
    models_dir: PathBuf,
    worker_rx: Option<Receiver<WorkerMessage>>,
    progress: Progress,
    operation: Option<&'static str>,
    heading: String,
}

impl Catalog {
    pub fn new(models_dir: PathBuf) -> Self {
        let entries = starter_entries(&models_dir);
        Self {
            entries,
            models_dir,
            worker_rx: None,
            progress: Progress::default(),
            operation: None,
            heading: "CURATED STARTERS".into(),
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub const fn busy(&self) -> bool {
        self.worker_rx.is_some()
    }

    pub const fn operation(&self) -> Option<&'static str> {
        self.operation
    }

    pub const fn progress(&self) -> Progress {
        self.progress
    }

    pub fn heading(&self) -> &str {
        &self.heading
    }

    pub fn restore_starters(&mut self) -> bool {
        if self.busy() {
            return false;
        }
        self.entries = starter_entries(&self.models_dir);
        self.heading = "CURATED STARTERS".into();
        true
    }

    pub fn check(&mut self) -> bool {
        if self.busy() {
            return false;
        }
        let repos = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (index, entry.repo.clone()))
            .collect::<Vec<_>>();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let results = repos
                .into_iter()
                .map(|(index, repo)| (index, verify_repo(&repo)))
                .collect();
            let _ = tx.send(WorkerMessage::Checked(results));
        });
        self.worker_rx = Some(rx);
        self.operation = Some("checking manifests");
        true
    }

    pub fn search(&mut self, query: &str) -> Result<bool, String> {
        if self.busy() {
            return Ok(false);
        }
        let query = query.split('·').next().unwrap_or(query).trim().to_string();
        if query.len() < 3 {
            return Err("the recommendation has no searchable model name".into());
        }
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = search_manifests(&query);
            let _ = tx.send(WorkerMessage::Searched { query, result });
        });
        self.worker_rx = Some(rx);
        self.operation = Some("finding Hugging Face artifacts");
        Ok(true)
    }

    pub fn download(&mut self, index: usize, available_bytes: u64) -> Result<bool, String> {
        if self.busy() {
            return Ok(false);
        }
        let entry = self
            .entries
            .get(index)
            .ok_or_else(|| "no Hugging Face model selected".to_string())?;
        let manifest = entry
            .manifest
            .clone()
            .ok_or_else(|| "check Hugging Face manifests before downloading".to_string())?;
        if manifest.total_bytes.saturating_add(DISK_RESERVE_BYTES) > available_bytes {
            return Err(format!(
                "download needs {} plus 1 GiB free",
                format_bytes(manifest.total_bytes)
            ));
        }
        let label = entry.label.clone();
        let total_bytes = manifest.total_bytes;
        let models_dir = self.models_dir.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let progress_tx = tx.clone();
            let result = download_manifest(&manifest, &models_dir, |downloaded, total| {
                let _ = progress_tx.send(WorkerMessage::Progress(Progress {
                    downloaded_bytes: downloaded,
                    total_bytes: total,
                }));
            });
            let _ = tx.send(WorkerMessage::Downloaded {
                index,
                label,
                result,
            });
        });
        self.progress = Progress {
            downloaded_bytes: 0,
            total_bytes,
        };
        self.worker_rx = Some(rx);
        self.operation = Some("downloading");
        Ok(true)
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut messages = Vec::new();
        let Some(rx) = self.worker_rx.as_ref() else {
            return Vec::new();
        };
        loop {
            match rx.try_recv() {
                Ok(message) => messages.push(message),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    messages.push(WorkerMessage::Stopped);
                    break;
                }
            }
        }

        let mut events = Vec::new();
        for message in messages {
            match message {
                WorkerMessage::Checked(results) => {
                    let mut checked = 0;
                    for (index, result) in results {
                        if let Some(entry) = self.entries.get_mut(index) {
                            match result {
                                Ok(manifest) => {
                                    entry.manifest = Some(manifest);
                                    entry.error = None;
                                    checked += 1;
                                }
                                Err(error) => entry.error = Some(error),
                            }
                        }
                    }
                    self.worker_rx = None;
                    self.operation = None;
                    events.push(Event::Checked(checked));
                }
                WorkerMessage::Searched { query, result } => {
                    self.worker_rx = None;
                    self.operation = None;
                    match result {
                        Ok(manifests) if !manifests.is_empty() => {
                            self.entries = manifests
                                .into_iter()
                                .map(|manifest| Entry {
                                    installed: install_path(&self.models_dir, &manifest.repo)
                                        .join("config.json")
                                        .is_file(),
                                    label: manifest
                                        .repo
                                        .split('/')
                                        .next_back()
                                        .unwrap_or(&manifest.repo)
                                        .to_string(),
                                    repo: manifest.repo.clone(),
                                    purpose: format!("Hugging Face match for {query}"),
                                    manifest: Some(manifest),
                                    error: None,
                                })
                                .collect();
                            self.heading = "SEARCH RESULTS".into();
                            events.push(Event::Searched(self.entries.len()));
                        }
                        Ok(_) => events.push(Event::Failed(format!(
                            "no compatible public safetensors repository matched {query}"
                        ))),
                        Err(error) => events.push(Event::Failed(error)),
                    }
                }
                WorkerMessage::Progress(progress) => self.progress = progress,
                WorkerMessage::Downloaded {
                    index,
                    label,
                    result,
                } => {
                    self.worker_rx = None;
                    self.operation = None;
                    match result {
                        Ok(target) => {
                            if let Some(entry) = self.entries.get_mut(index) {
                                entry.installed = true;
                            }
                            events.push(Event::Downloaded { label, target });
                        }
                        Err(error) => events.push(Event::Failed(error)),
                    }
                }
                WorkerMessage::Stopped => {
                    self.worker_rx = None;
                    self.operation = None;
                    events.push(Event::Failed("Hugging Face worker stopped".into()));
                }
            }
        }
        events
    }
}

enum WorkerMessage {
    Checked(Vec<(usize, Result<Manifest, String>)>),
    Searched {
        query: String,
        result: Result<Vec<Manifest>, String>,
    },
    Progress(Progress),
    Downloaded {
        index: usize,
        label: String,
        result: Result<PathBuf, String>,
    },
    Stopped,
}

pub fn starters() -> impl Iterator<Item = &'static Candidate> {
    STARTERS.iter().filter(|candidate| {
        candidate.repo.starts_with("mlx-community/") == cfg!(target_os = "macos")
    })
}

fn starter_entries(models_dir: &Path) -> Vec<Entry> {
    starters()
        .map(|candidate| Entry {
            installed: install_path(models_dir, candidate.repo)
                .join("config.json")
                .is_file(),
            repo: candidate.repo.into(),
            label: candidate.label.into(),
            purpose: candidate.purpose.into(),
            manifest: None,
            error: None,
        })
        .collect()
}

pub fn search_manifests(query: &str) -> Result<Vec<Manifest>, String> {
    let mut request = ureq::get(API_ROOT)
        .query("search", query)
        .query("limit", "12")
        .query("full", "true");
    if cfg!(target_os = "macos") {
        request = request.query("author", "mlx-community");
    }
    let response = request
        .timeout(Duration::from_secs(20))
        .call()
        .map_err(|error| format!("Hugging Face search failed: {error}"))?;
    let rows = response
        .into_json::<Vec<serde_json::Value>>()
        .map_err(|error| format!("Hugging Face search returned invalid JSON: {error}"))?;
    let repos = rows
        .into_iter()
        .filter(|row| row["private"].as_bool() != Some(true))
        .filter(|row| {
            matches!(
                row.get("gated"),
                None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false))
            )
        })
        .filter(|row| row["pipeline_tag"].as_str() == Some("text-generation"))
        .filter(|row| {
            let tags = row["tags"].as_array();
            let has = |needle: &str| {
                tags.is_some_and(|tags| tags.iter().any(|tag| tag.as_str() == Some(needle)))
            };
            has("safetensors") && (!cfg!(target_os = "macos") || has("mlx"))
        })
        .filter_map(|row| row["id"].as_str().map(str::to_string))
        .collect::<Vec<_>>();

    let mut manifests = Vec::new();
    for repo in repos.into_iter().take(5) {
        if let Ok(manifest) = verify_repo(&repo) {
            manifests.push(manifest);
        }
    }
    Ok(manifests)
}

pub fn verify_repo(repo: &str) -> Result<Manifest, String> {
    validate_repo(repo)?;
    let response = ureq::get(&format!("{API_ROOT}/{repo}?blobs=true"))
        .timeout(Duration::from_secs(20))
        .call()
        .map_err(|error| format!("Hugging Face check failed for {repo}: {error}"))?;
    let value = response
        .into_json::<serde_json::Value>()
        .map_err(|error| format!("Hugging Face returned invalid JSON for {repo}: {error}"))?;
    manifest_from_value(repo, &value)
}

fn manifest_from_value(repo: &str, value: &serde_json::Value) -> Result<Manifest, String> {
    if value["id"].as_str() != Some(repo) {
        return Err("repository identity did not match the request".into());
    }
    if value["private"].as_bool() == Some(true) {
        return Err("private repositories are not supported".into());
    }
    if !matches!(
        value.get("gated"),
        None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false))
    ) {
        return Err("gated repositories require browser authentication".into());
    }
    let revision = value["sha"]
        .as_str()
        .filter(|revision| {
            revision.len() == 40
                && revision
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
        })
        .ok_or_else(|| "repository did not return an immutable commit".to_string())?
        .to_string();
    let siblings = value["siblings"]
        .as_array()
        .ok_or_else(|| "repository did not return a file manifest".to_string())?;
    let mut files = Vec::new();
    let mut has_config = false;
    let mut has_tokenizer = false;
    let mut has_weights = false;
    for sibling in siblings {
        let Some(name) = sibling["rfilename"].as_str() else {
            continue;
        };
        if !supported_file(name) {
            continue;
        }
        validate_relative_file(name)?;
        let size = sibling["size"]
            .as_u64()
            .or_else(|| sibling["lfs"]["size"].as_u64())
            .ok_or_else(|| format!("{name} has no reported size"))?;
        let sha256 = sibling["lfs"]["sha256"].as_str().map(str::to_string);
        if name.ends_with(".safetensors") {
            has_weights = true;
            if sha256.as_deref().is_none_or(|hash| {
                hash.len() != 64 || !hash.chars().all(|character| character.is_ascii_hexdigit())
            }) {
                return Err(format!("{name} has no valid LFS SHA-256"));
            }
        }
        has_config |= name == "config.json";
        has_tokenizer |= name.starts_with("tokenizer") || name == "vocab.json";
        files.push(RemoteFile {
            name: name.to_string(),
            size,
            sha256,
        });
    }
    if !has_config || !has_tokenizer || !has_weights {
        return Err("repository needs config, tokenizer, and safetensors files".into());
    }
    let total_bytes = files.iter().map(|file| file.size).sum();
    Ok(Manifest {
        repo: repo.to_string(),
        revision,
        total_bytes,
        files,
    })
}

pub fn download_manifest(
    manifest: &Manifest,
    models_dir: &Path,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<PathBuf, String> {
    validate_repo(&manifest.repo)?;
    let target = install_path(models_dir, &manifest.repo);
    if target.join("config.json").is_file() {
        return Ok(target);
    }
    fs::create_dir_all(models_dir).map_err(|error| error.to_string())?;
    let staging = models_dir.join(format!(
        ".tokoro-download-{}-{}",
        manifest.repo.replace('/', "--"),
        std::process::id()
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| error.to_string())?;
    }
    fs::create_dir(&staging).map_err(|error| error.to_string())?;

    let result = (|| {
        let mut downloaded_total = 0;
        for remote in &manifest.files {
            validate_relative_file(&remote.name)?;
            let url = format!(
                "{DOWNLOAD_ROOT}/{}/resolve/{}/{}",
                manifest.repo, manifest.revision, remote.name
            );
            let response = ureq::get(&url)
                .timeout(Duration::from_secs(120))
                .call()
                .map_err(|error| format!("download failed for {}: {error}", remote.name))?;
            let path = staging.join(&remote.name);
            let mut output = fs::File::create(&path).map_err(|error| error.to_string())?;
            let mut input = response.into_reader();
            let mut hasher = Sha256::new();
            let mut file_bytes = 0;
            let mut buffer = vec![0_u8; 1024 * 1024];
            loop {
                let read = input.read(&mut buffer).map_err(|error| error.to_string())?;
                if read == 0 {
                    break;
                }
                output
                    .write_all(&buffer[..read])
                    .map_err(|error| error.to_string())?;
                hasher.update(&buffer[..read]);
                file_bytes += read as u64;
                on_progress(downloaded_total + file_bytes, manifest.total_bytes);
            }
            output.sync_all().map_err(|error| error.to_string())?;
            if file_bytes != remote.size {
                return Err(format!(
                    "{} size mismatch: expected {}, received {}",
                    remote.name, remote.size, file_bytes
                ));
            }
            if let Some(expected) = &remote.sha256 {
                let actual = format!("{:x}", hasher.finalize());
                if &actual != expected {
                    return Err(format!("{} SHA-256 mismatch", remote.name));
                }
            }
            downloaded_total += file_bytes;
        }
        let receipt = serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?;
        fs::write(staging.join("tokoro-manifest.json"), receipt)
            .map_err(|error| error.to_string())?;
        fs::rename(&staging, &target).map_err(|error| error.to_string())?;
        Ok(target.clone())
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

pub fn install_path(models_dir: &Path, repo: &str) -> PathBuf {
    models_dir.join(repo.replace('/', "--"))
}

fn validate_repo(repo: &str) -> Result<(), String> {
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty()
        || name.is_empty()
        || parts.next().is_some()
        || !owner.chars().all(repo_character)
        || !name.chars().all(repo_character)
    {
        return Err("expected a Hugging Face repository in owner/name form".into());
    }
    Ok(())
}

fn repo_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

fn validate_relative_file(name: &str) -> Result<(), String> {
    let path = Path::new(name);
    if name.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("unsafe repository file name: {name}"));
    }
    Ok(())
}

fn supported_file(name: &str) -> bool {
    !name.contains('/')
        && (name == "config.json"
            || name == "generation_config.json"
            || name == "tokenizer.json"
            || name == "tokenizer_config.json"
            || name == "special_tokens_map.json"
            || name == "added_tokens.json"
            || name == "vocab.json"
            || name == "merges.txt"
            || name == "chat_template.jinja"
            || name == "model.safetensors.index.json"
            || name.ends_with(".safetensors"))
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes >= BYTES_PER_GIB {
        format!("{:.1} GiB", bytes as f64 / BYTES_PER_GIB as f64)
    } else {
        format!("{:.0} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> serde_json::Value {
        serde_json::json!({
            "id": "org/tiny-model",
            "sha": "0123456789abcdef0123456789abcdef01234567",
            "private": false,
            "gated": false,
            "siblings": [
                {"rfilename": "config.json", "size": 100},
                {"rfilename": "tokenizer.json", "size": 200},
                {"rfilename": "model.safetensors", "size": 300, "lfs": {
                    "size": 300,
                    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                }}
            ]
        })
    }

    #[test]
    fn accepts_a_pinned_public_safetensors_manifest() {
        let manifest =
            manifest_from_value("org/tiny-model", &fixture()).expect("valid manifest should pass");
        assert_eq!(manifest.total_bytes, 600);
        assert_eq!(manifest.files.len(), 3);
    }

    #[test]
    fn rejects_gated_or_unhashed_weights() {
        let mut gated = fixture();
        gated["gated"] = serde_json::json!("manual");
        assert!(manifest_from_value("org/tiny-model", &gated).is_err());

        let mut unhashed = fixture();
        unhashed["siblings"][2]["lfs"]["sha256"] = serde_json::Value::Null;
        assert!(manifest_from_value("org/tiny-model", &unhashed).is_err());
    }

    #[test]
    fn rejects_repository_path_traversal() {
        assert!(validate_relative_file("../model.safetensors").is_err());
        assert!(validate_repo("org/../../model").is_err());
    }
}
