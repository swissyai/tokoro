// tokoro v0.8.0, a place for local models
// Telemetry + stages + benchmarks + server mgmt + harness snippets + interference.
// The lower of model context and memory capacity sets the context gauge.
// Theme-native (ANSI) by default; Ghostty theme files via config.

mod agents;
mod bloat;
mod cli;
mod commands;
mod device;
mod eval;
mod handoff;
mod huggingface;
mod input;
mod intro;
mod learn;
mod local_ai;
mod monitoring;
mod platform;
mod report;
mod runtime;
mod settings;
mod ui;

use commands::Action as CommandAction;
use crossterm::{
    event::{self, Event, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use runtime::{EngineMetrics, ModelSource, RoundEvent, ServedModel};
use settings::{expand_home, load_config, save_config, theme_choices, Config, ServerConfig, Theme};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const MAX_REQUESTS_HARD: usize = 128;
const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const BYTES_PER_MIB: f64 = 1024.0 * 1024.0;
const MAX_OFFENDERS: usize = 12;

const DENYLIST: &[&str] = &[
    "tokoro",
    "kernel_task",
    "launchd",
    "WindowServer",
    "loginwindow",
    "SystemUIServer",
    "systemd",
    "init",
    "sshd",
    "Terminal",
    "iTerm2",
    "ghostty",
    "alacritty",
    "foot",
    "zsh",
    "bash",
    "fish",
    "pi",
    "opencode",
    "claude",
    "Finder",
];

fn hint_for(name: &str) -> &'static str {
    match name {
        n if n.contains("mds") => "spotlight indexing",
        n if n.contains("backupd") => "time machine backup",
        n if n.contains("photoanalysis") || n.contains("mediaanalysis") => "photo library analysis",
        n if n.contains("docker") || n.contains("vmnetd") || n.contains("com.docker") => {
            "docker vm"
        }
        _ => "",
    }
}

// ────────────────────── Model architecture truth ──────────────────────
// Read the model's real config.json from the HF cache: KV bytes/token and
// max context come from the architecture, not from name-parsing guesses.

#[derive(Clone, Copy)]
struct ModelArch {
    kv_bytes_per_token: f64,
    max_context: u32,
}

fn find_model_config(models_dir: &str, model: &str) -> Option<PathBuf> {
    // Managed local models are often exposed through a friendly symlink, while the
    // model id returned by /v1/models is only its basename. Try both forms before
    // falling back to the Hugging Face cache layout.
    let models_root = expand_home(models_dir);
    let candidates = [
        expand_home(model).join("config.json"),
        models_root.join(model).join("config.json"),
    ];
    for cfg in candidates {
        if cfg.is_file() {
            return Some(cfg);
        }
    }

    let dir_name = format!("models--{}", model.replace('/', "--"));
    let base = models_root.join(&dir_name).join("snapshots");
    if let Ok(entries) = fs::read_dir(&base) {
        for e in entries.flatten() {
            let cfg = e.path().join("config.json");
            if cfg.is_file() {
                return Some(cfg);
            }
        }
    }
    None
}

fn read_model_arch(models_dir: &str, model: &str) -> Option<ModelArch> {
    let path = find_model_config(models_dir, model)?;
    let text = fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    // Qwen3.5/Qwen3.8 and other multimodal checkpoints keep the text
    // architecture under text_config. Plain text checkpoints use the root.
    let c = v
        .get("text_config")
        .filter(|value| value.is_object())
        .unwrap_or(&v);

    let layers = c["num_hidden_layers"].as_f64()?;
    let kv_heads = c["num_key_value_heads"]
        .as_f64()
        .or(c["num_attention_heads"].as_f64())?;
    let head_dim = c["head_dim"].as_f64().or_else(|| {
        let hidden = c["hidden_size"].as_f64()?;
        let heads = c["num_attention_heads"].as_f64()?;
        Some(hidden / heads)
    })?;
    let dtype = c["dtype"]
        .as_str()
        .or(v["torch_dtype"].as_str())
        .unwrap_or("bfloat16");
    let dtype_bytes = match dtype {
        "float32" | "fp32" => 4.0,
        "float16" | "bfloat16" | "fp16" | "bf16" => 2.0,
        _ => 2.0,
    };
    // Hybrid models only grow a conventional KV cache on their full-attention
    // layers. Counting every layer makes a Qwen3.8 context gauge 4x too small.
    let attention_layers = c["layer_types"]
        .as_array()
        .map(|types| {
            types
                .iter()
                .filter(|kind| matches!(kind.as_str(), Some("full_attention") | Some("attention")))
                .count() as f64
        })
        .filter(|count| *count > 0.0)
        .or_else(|| {
            c["full_attention_interval"]
                .as_f64()
                .filter(|interval| *interval > 0.0)
                .map(|interval| (layers / interval).max(1.0))
        })
        .unwrap_or(layers);
    let kv = 2.0 * attention_layers * kv_heads * head_dim * dtype_bytes; // K and V
    let max_ctx = c["max_position_embeddings"]
        .as_u64()
        .or(v["max_position_embeddings"].as_u64())
        .unwrap_or(32768) as u32;
    Some(ModelArch {
        kv_bytes_per_token: kv / (1024.0 * 1024.0),
        max_context: max_ctx,
    })
}

// ─────────────────────── Stage state machine ───────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Queued,
    Prefill,
    Decode,
    Done,
    Failed,
}

#[derive(Clone)]
struct RequestSpan {
    id: String,
    started: Instant,
    prompt_tokens: u32,
    prefill_done: u32,
    prefill_rate: f64,
    first_token: Option<Instant>,
    decoded: u32,
    decode_rate: f64,
    cached_tokens: Option<u32>,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    top_k: Option<u32>,
    stage: Stage,
    last_update: Instant,
}

impl RequestSpan {
    fn new(id: String) -> Self {
        Self {
            id,
            started: Instant::now(),
            prompt_tokens: 0,
            prefill_done: 0,
            prefill_rate: 0.0,
            first_token: None,
            decoded: 0,
            decode_rate: 0.0,
            cached_tokens: None,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stage: Stage::Queued,
            last_update: Instant::now(),
        }
    }
    fn prefill_eta(&self) -> Option<Duration> {
        if self.prefill_rate > 1.0 && self.prompt_tokens > self.prefill_done {
            Some(Duration::from_secs_f64(
                (self.prompt_tokens - self.prefill_done) as f64 / self.prefill_rate,
            ))
        } else {
            None
        }
    }
    fn ttft(&self) -> Option<Duration> {
        self.first_token.map(|t| t - self.started)
    }
    fn cache_hit_ratio(&self) -> Option<f64> {
        if self.prompt_tokens > 0 {
            self.cached_tokens
                .map(|c| c as f64 / self.prompt_tokens as f64)
        } else {
            None
        }
    }

    fn sampling_summary(&self) -> String {
        let mut values = Vec::new();
        if let Some(temperature) = self.temperature {
            values.push(format!("temp {temperature:.2}"));
        }
        if let Some(top_p) = self.top_p {
            values.push(format!("top-p {top_p:.2}"));
        }
        if let Some(top_k) = self.top_k {
            values.push(format!("top-k {top_k}"));
        }
        if let Some(max_tokens) = self.max_tokens {
            values.push(format!("max {max_tokens}"));
        }
        if values.is_empty() {
            "not reported".into()
        } else {
            values.join(" | ")
        }
    }
}

// ─────────────────────────── Log follower ───────────────────────────

struct LogFollower {
    path: PathBuf,
    offset: u64,
}

impl LogFollower {
    fn new(path: &str) -> Option<Self> {
        let expanded = expand_home(path);
        let f = fs::File::open(&expanded).ok()?;
        let offset = f.metadata().ok()?.len();
        Some(Self {
            path: expanded,
            offset,
        })
    }

    fn poll(
        &mut self,
        spans: &mut VecDeque<RequestSpan>,
        current: &mut Option<RequestSpan>,
    ) -> bool {
        let Ok(mut f) = fs::File::open(&self.path) else {
            return false;
        };
        let Ok(meta) = f.metadata() else { return false };
        if meta.len() < self.offset {
            self.offset = 0;
        }
        if meta.len() == self.offset {
            return false;
        }
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return false;
        }
        let mut buf = String::new();
        if f.read_to_string(&mut buf).is_err() {
            return false;
        }
        self.offset += buf.len() as u64;
        let mut active = false;
        for line in buf.lines() {
            if Self::parse_line(line, spans, current) {
                active = true;
            }
        }
        active
    }

    fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
        line.find(key).map(|i| {
            line[i + key.len()..]
                .split_whitespace()
                .next()
                .unwrap_or("")
        })
    }

    fn parse_line(
        line: &str,
        spans: &mut VecDeque<RequestSpan>,
        current: &mut Option<RequestSpan>,
    ) -> bool {
        if line.contains("Generation queued:") {
            let id = Self::field(line, "request=").unwrap_or("?").to_string();
            let mut span = RequestSpan::new(id);
            span.prompt_tokens = Self::field(line, "prompt_tokens=")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            span.max_tokens = Self::field(line, "max_tokens=").and_then(|v| v.parse().ok());
            span.temperature = Self::field(line, "temperature=").and_then(|v| v.parse().ok());
            span.top_p = Self::field(line, "top_p=").and_then(|v| v.parse().ok());
            span.top_k = Self::field(line, "top_k=").and_then(|v| v.parse().ok());
            *current = Some(span);
            return true;
        }
        if line.contains("Decode started:") {
            if let Some(c) = current.as_mut() {
                if let Some(ttft) = Self::field(line, "time_to_first_token=")
                    .and_then(|value| value.trim_end_matches('s').parse::<f64>().ok())
                {
                    c.first_token = Some(c.started + Duration::from_secs_f64(ttft));
                } else if c.first_token.is_none() {
                    c.first_token = Some(Instant::now());
                }
                c.stage = Stage::Decode;
                c.last_update = Instant::now();
            }
            return true;
        }
        if line.contains("Decode progress:") {
            if let Some(c) = current.as_mut() {
                c.stage = Stage::Decode;
                if let Some(tokens) = Self::field(line, "generated_tokens=")
                    .and_then(|value| value.parse::<u32>().ok())
                {
                    c.decoded = tokens;
                }
                if let Some(rate) =
                    Self::field(line, "rate=").and_then(|value| value.parse::<f64>().ok())
                {
                    c.decode_rate = rate;
                }
                if c.first_token.is_none() {
                    c.first_token = Some(Instant::now());
                }
                c.last_update = Instant::now();
            }
            return true;
        }
        if line.contains("Prefill completed:") || line.contains("Prefill finished:") {
            if let Some(c) = current.as_mut() {
                if let Some(cached) = Self::field(line, "cached_tokens=") {
                    if let Ok(v) = cached.parse() {
                        c.cached_tokens = Some(v);
                    }
                }
                if c.prefill_done == 0 {
                    c.prefill_done = c.prompt_tokens;
                }
                c.first_token = Some(Instant::now());
                if c.stage == Stage::Prefill {
                    c.stage = Stage::Decode;
                }
            }
            return true;
        }
        if line.contains("Prefill started:") {
            if let Some(c) = current.as_mut() {
                c.stage = Stage::Prefill;
                c.last_update = Instant::now();
                if c.prompt_tokens == 0 {
                    c.prompt_tokens = Self::field(line, "prompt_tokens=")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                }
            }
            return true;
        }
        if line.contains("Prefill progress:") {
            if let Some(c) = current.as_mut() {
                if let Some(tok) = Self::field(line, "tokens=") {
                    if let Some((done, _)) = tok.split_once('/') {
                        if let Ok(done) = done.parse::<u32>() {
                            let now = Instant::now();
                            let dt = (now - c.last_update).as_secs_f64();
                            if dt > 0.1 && done > c.prefill_done {
                                let rate = (done - c.prefill_done) as f64 / dt;
                                c.prefill_rate = 0.5 * c.prefill_rate + 0.5 * rate;
                            }
                            c.prefill_done = done;
                            if c.first_token.is_none() {
                                c.first_token = Some(now);
                            }
                            c.last_update = now;
                            c.stage = Stage::Prefill;
                        }
                    }
                }
            }
            return true;
        }
        if line.contains("Generation cancelled:") || line.contains("Request failed:") {
            if let Some(mut c) = current.take() {
                c.stage = Stage::Failed;
                spans.push_back(c);
                if spans.len() > MAX_REQUESTS_HARD {
                    spans.pop_front();
                }
            }
            return false;
        }
        if line.contains("Generation complete")
            || line.contains("Request completed")
            || line.contains("Generation finished")
        {
            if let Some(mut c) = current.take() {
                c.stage = Stage::Done;
                c.decoded = Self::field(line, "generated_tokens=")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                spans.push_back(c);
                if spans.len() > MAX_REQUESTS_HARD {
                    spans.pop_front();
                }
            }
            return false;
        }
        false
    }
}

// ─────────────────────── Server management ───────────────────────

#[derive(Clone)]
struct ModelChoice {
    target: String,
    label: String,
    detail: String,
    can_start: bool,
}

struct ServerManager {
    child: Option<Child>,
    available: Vec<ModelChoice>,
    catalog_rx: Option<mpsc::Receiver<Vec<ModelChoice>>>,
    managed_available: bool,
    managed_accepts_safetensors: bool,
}

impl ServerManager {
    fn new(cfg: &ServerConfig) -> Self {
        let managed_available = !cfg.command.trim().is_empty();
        let managed_accepts_safetensors =
            managed_available && !cfg.command.to_ascii_lowercase().contains("llama-server");
        let local = discover_models(cfg);
        let available: Vec<ModelChoice> = local
            .into_iter()
            .map(|target| {
                let can_start = managed_target_compatible(&cfg.command, &target);
                ModelChoice {
                    label: model_display_label(&target),
                    target,
                    detail: if can_start {
                        "local target; managed server available".into()
                    } else if managed_available {
                        "local target; incompatible with configured managed server".into()
                    } else {
                        "local target; observe with Ollama or configure a server".into()
                    },
                    can_start,
                }
            })
            .collect();
        let catalog_cfg = cfg.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(discover_catalog(&catalog_cfg));
        });
        Self {
            child: None,
            available,
            catalog_rx: Some(rx),
            managed_available,
            managed_accepts_safetensors,
        }
    }

    fn poll_catalog(&mut self) -> bool {
        let Some(rx) = self.catalog_rx.as_ref() else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(Vec::new()),
            Err(mpsc::TryRecvError::Empty) => None,
        };
        let Some(catalog) = result else {
            return false;
        };
        self.catalog_rx = None;
        let mut seen: HashSet<String> = self
            .available
            .iter()
            .map(|choice| choice.target.clone())
            .collect();
        for choice in catalog {
            if seen.insert(choice.target.clone()) {
                self.available.push(choice);
            }
        }
        true
    }

    fn catalog_loading(&self) -> bool {
        self.catalog_rx.is_some()
    }

    fn add_local_target(&mut self, label: &str, target: &Path) {
        let target = target.to_string_lossy().into_owned();
        if self.available.iter().any(|choice| choice.target == target) {
            return;
        }
        self.available.push(ModelChoice {
            target,
            label: label.into(),
            detail: if self.managed_accepts_safetensors {
                "downloaded; checked; managed server available".into()
            } else {
                "downloaded and checked; use a safetensors-compatible runtime".into()
            },
            can_start: self.managed_accepts_safetensors,
        });
    }

    fn running(&self) -> bool {
        self.child.is_some()
    }

    fn start(&mut self, cfg: &ServerConfig, model: &str, log_path: &str) -> Result<(), String> {
        if self.child.is_some() {
            return Err("already running".into());
        }
        if cfg.command.trim().is_empty() {
            return Err(
                "no managed server configured; Tokoro can still observe Ollama and local APIs"
                    .into(),
            );
        }
        let args = server_arguments(&cfg.args, model, cfg.port)?;
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(expand_home(log_path))
            .map_err(|e| e.to_string())?;
        let log_err = log.try_clone().map_err(|e| e.to_string())?;
        let command = expand_home(&cfg.command);
        let child = Command::new(&command)
            .args(args)
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|e| format!("spawn {}: {}", command.display(), e))?;
        self.child = Some(child);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn managed_target_compatible(command: &str, target: &str) -> bool {
    if command.trim().is_empty() {
        return false;
    }
    if command.to_ascii_lowercase().contains("llama-server") {
        return Path::new(target)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"));
    }
    true
}

fn server_arguments(template: &str, model: &str, port: u16) -> Result<Vec<String>, String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for character in template.chars() {
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            } else {
                current.push(character);
            }
            continue;
        }
        if character.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                arguments.push(current);
                current = String::new();
            }
        } else {
            current.push(character);
        }
    }
    if quote.is_some() {
        return Err("server argument template has an unclosed quote".into());
    }
    if !current.is_empty() {
        arguments.push(current);
    }
    Ok(arguments
        .into_iter()
        .map(|argument| {
            argument
                .replace("{model}", model)
                .replace("{port}", &port.to_string())
        })
        .collect())
}

fn discover_catalog(cfg: &ServerConfig) -> Vec<ModelChoice> {
    if cfg.command.trim().is_empty() {
        return Vec::new();
    }
    let command = expand_home(&cfg.command);
    let Ok(output) = Command::new(command)
        .args(["doctor", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let Ok(root) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return Vec::new();
    };
    let Some(rows) = root["models"].as_array() else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let target = row["target"].as_str()?.to_string();
            let fits = row["fits"].as_bool();
            let ready = row["ready"].as_bool().unwrap_or(false);
            let target_installed = row["target_installed"].as_bool().unwrap_or(false);
            let detail = if fits == Some(false) {
                "over RAM".to_string()
            } else if ready {
                "ready".to_string()
            } else if target_installed {
                "target local; drafter downloads".to_string()
            } else {
                "download on start".to_string()
            };
            Some(ModelChoice {
                label: target.clone(),
                target,
                detail,
                can_start: fits != Some(false),
            })
        })
        .collect()
}

fn discover_models(cfg: &ServerConfig) -> Vec<String> {
    let dir = expand_home(&cfg.models_dir);
    let mut out: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
            {
                out.push(path.to_string_lossy().into_owned());
                continue;
            }
            if let Some(rest) = name.strip_prefix("models--") {
                out.push(rest.replace("--", "/"));
                continue;
            }
            let cfg_here = path.join("config.json");
            let cfg_8bit = path.join("8-bit").join("config.json");
            if cfg_here.is_file() {
                out.push(path.to_string_lossy().into_owned());
            } else if cfg_8bit.is_file() {
                out.push(path.join("8-bit").to_string_lossy().into_owned());
            }
        }
    }
    // A local model may be present both as a friendly symlink and as a nested
    // precision directory (for example qwen3.8-27b-uncensored-8bit and 8-bit).
    // Deduplicate by canonical path and keep the most descriptive family name,
    // so mlx-dspark can auto-resolve its matched drafter from the basename.
    let priority = model_priority;
    let mut unique: HashMap<PathBuf, String> = HashMap::new();
    for candidate in out {
        let key = fs::canonicalize(&candidate).unwrap_or_else(|_| PathBuf::from(&candidate));
        let replace = unique
            .get(&key)
            .map(|current| priority(&candidate) < priority(current))
            .unwrap_or(true);
        if replace {
            unique.insert(key, candidate);
        }
    }
    let mut out: Vec<String> = unique.into_values().collect();
    out.sort();
    out
}

fn model_display_label(model: &str) -> String {
    let path = PathBuf::from(model);
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| model.to_string());
    if matches!(
        name.to_lowercase().as_str(),
        "4-bit" | "8-bit" | "bf16" | "fp16"
    ) {
        if let Some(parent) = path.parent().and_then(|parent| parent.file_name()) {
            return format!("{}/{}", parent.to_string_lossy(), name);
        }
    }
    name
}

fn model_priority(model: &str) -> (bool, bool, Reverse<usize>) {
    let name = PathBuf::from(model)
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let generic_precision = matches!(name.as_str(), "4-bit" | "8-bit" | "bf16" | "fp16");
    let informative = name.contains("qwen") || name.contains("gemma") || name.contains("llama");
    // Prefer a non-generic, family-bearing alias. Among aliases for the same
    // checkpoint, the longer name carries more information for auto-routing.
    (generic_precision, !informative, Reverse(model.len()))
}

// ─────────────────────── Harness snippets ───────────────────────

fn harness_snippets(model: &str, port: u16) -> Vec<(&'static str, String)> {
    let base = format!("http://127.0.0.1:{}/v1", port);
    vec![
        (
            "pi",
            format!(
                r#"# ~/.pi/agent/models.json
{{
  "providers": {{
    "tokoro-local": {{
      "baseUrl": "{base}",
      "api": "openai-completions",
      "apiKey": "local",
      "models": [{{ "id": "{model}", "contextWindow": 32768, "input": ["text"] }}]
    }}
  }}
}}

export PI_TIMEOUT=600000
pi --provider tokoro-local --model "{model}""#
            ),
        ),
        (
            "OpenCode",
            format!(
                r#"// opencode.json
{{
  "$schema": "https://opencode.ai/config.json",
  "provider": {{
    "tokoro-local": {{
      "npm": "@ai-sdk/openai-compatible",
      "name": "tokoro local",
      "options": {{ "baseURL": "{base}" }},
      "models": {{ "{model}": {{ "name": "{model}" }} }}
    }}
  }}
}}"#
            ),
        ),
        (
            "Codex CLI",
            format!(
                r#"# ~/.codex/config.toml
model = "{model}"
model_provider = "tokoro-local"

[model_providers.tokoro-local]
name = "tokoro local"
base_url = "{base}"
env_key = "OPENAI_API_KEY"
stream_idle_timeout_ms = 600000   # survive long local prefill"#
            ),
        ),
        (
            "mlx-dspark",
            format!(
                r#"# local speculative decode on the same weights
# installed at ~/models/.dspark-venv
~/models/.dspark-venv/bin/mlx-dspark serve \\
  --mode auto --no-thinking --model {model} --port {port}
# Qwen3.8-27B auto-resolves DFlash 2; target verifies every token.
# OpenAI base: {base}
# tokoro idle (no server) is the non-LLM baseline: RAM/CPU before weights load"#
            ),
        ),
        (
            "Ollama",
            format!(
                r#"# Ollama's OpenAI-compatible API
# native API: http://127.0.0.1:{port}/api
base URL: {base}
model: {model}
api key: ollama"#
            ),
        ),
        (
            "LM Studio",
            format!(
                r#"# LM Studio local server
# default port: 1234
base URL: {base}
model: {model}
api key: lm-studio"#
            ),
        ),
        (
            "llama.cpp",
            format!(
                r#"# llama-server OpenAI-compatible API
# default port: 8080
base URL: {base}
model: {model}
api key: local"#
            ),
        ),
        (
            "OpenAI-compatible",
            format!(
                r#"# Any local runtime exposing /v1/models and /v1/chat/completions
base URL: {base}
model: {model}
api key: local"#
            ),
        ),
        (
            "Claude Code",
            format!(
                r#"# Claude Code speaks Anthropic protocol - route via a proxy
# (e.g. claude-code-proxy or LiteLLM in front of {base})
export ANTHROPIC_BASE_URL="http://localhost:8082"
export ANTHROPIC_AUTH_TOKEN="sk-local"
claude"#
            ),
        ),
        (
            "Aider",
            format!(
                r#"aider --openai-api-base {base} \
      --openai-api-key local \
      --model openai/{model}"#
            ),
        ),
        (
            "Continue",
            format!(
                r#"// ~/.continue/config.yaml
name: Tokoro local
version: 0.0.1
models:
  - name: {model}
    provider: openai
    model: {model}
    apiBase: {base}"#
            ),
        ),
        (
            "Cursor",
            format!(
                r#"# Cursor / OpenAI-compatible endpoint
# Settings -> Models -> OpenAI API Base URL
base URL: {base}
model: {model}
API key: local"#
            ),
        ),
        (
            "Cline",
            format!(
                r#"# Cline custom OpenAI-compatible provider
base URL: {base}
model ID: {model}
API key: local"#
            ),
        ),
        (
            "Roo Code",
            format!(
                r#"# Roo Code custom OpenAI-compatible provider
base URL: {base}
model ID: {model}
API key: local"#
            ),
        ),
        (
            "Neovim",
            format!(
                r#"-- CodeCompanion / Avante OpenAI-compatible endpoint
local = {{
  __inherited_from = 'openai',
  endpoint = '{base}/chat/completions',
  model = '{model}',
  api_key = 'local',
}}"#
            ),
        ),
        (
            "OpenAI SDK",
            format!(
                r#"from openai import OpenAI
client = OpenAI(base_url='{base}', api_key='local')
response = client.chat.completions.create(
    model='{model}', messages=[{{'role': 'user', 'content': 'Hello'}}]
)"#
            ),
        ),
        (
            "curl",
            format!(
                r#"curl {base}/chat/completions \\
  -H 'Content-Type: application/json' \\
  -H 'Authorization: Bearer local' \\
  -d '{{"model":"{model}","messages":[{{"role":"user","content":"Hello"}}],"stream":true}}'"#
            ),
        ),
    ]
}

fn connection_description(name: &str) -> &'static str {
    match name {
        "pi" => "Pi coding agent",
        "OpenCode" => "OpenCode terminal agent",
        "Codex CLI" => "OpenAI-compatible Codex provider",
        "Claude Code" => "Anthropic client through a local proxy",
        "Aider" => "Aider coding assistant",
        "Continue" => "Continue editor extension",
        "Cursor" => "Cursor model settings",
        "Cline" => "Cline editor extension",
        "Roo Code" => "Roo Code editor extension",
        "Neovim" => "CodeCompanion or Avante",
        "OpenAI SDK" => "Python OpenAI client",
        "curl" => "raw HTTP smoke test",
        "mlx-dspark" => "MLX speculative local server command",
        "Ollama" => "Ollama OpenAI-compatible endpoint",
        "LM Studio" => "LM Studio local server endpoint",
        "llama.cpp" => "llama-server OpenAI-compatible endpoint",
        "OpenAI-compatible" => "generic local endpoint contract",
        _ => "OpenAI-compatible connection",
    }
}

fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    let query = query.trim().to_lowercase();
    let candidate = candidate.to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    if let Some(position) = candidate.find(&query) {
        return Some(10_000 - position as i32);
    }
    let mut score = 0;
    let mut cursor = 0;
    let mut previous = None;
    for needle in query.chars() {
        let offset = candidate[cursor..].find(needle)?;
        let position = cursor + offset;
        score += 100 - position as i32;
        if previous == Some(position.saturating_sub(1)) {
            score += 25;
        }
        previous = Some(position);
        cursor = position + needle.len_utf8();
    }
    Some(score)
}

fn theme_matches(app: &App) -> Vec<usize> {
    let mut matches = app
        .theme_choices
        .iter()
        .enumerate()
        .filter_map(|(index, name)| fuzzy_score(&app.theme_query, name).map(|score| (index, score)))
        .collect::<Vec<_>>();
    matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    matches.into_iter().map(|(index, _)| index).collect()
}

fn connection_matches(app: &App) -> Vec<usize> {
    let snippets = harness_snippets(&app.connect_model, app.port);
    let mut matches: Vec<(usize, bool, bool, i32)> = snippets
        .iter()
        .enumerate()
        .filter_map(|(index, (name, _))| {
            let candidate = format!("{} {}", name, connection_description(name));
            let score = fuzzy_score(&app.connect_query, &candidate)?;
            Some((
                index,
                app.agents.has(name),
                app.connect_favorites.contains(*name),
                score,
            ))
        })
        .collect();
    matches.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| b.3.cmp(&a.3))
            .then_with(|| a.0.cmp(&b.0))
    });
    matches.into_iter().map(|(index, _, _, _)| index).collect()
}

fn connection_favorites_path() -> PathBuf {
    platform::state_home()
        .join("tokoro")
        .join("connections-favorites.txt")
}

fn load_connection_favorites(defaults: &[String]) -> HashSet<String> {
    let path = connection_favorites_path();
    fs::read_to_string(path)
        .ok()
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect()
        })
        .filter(|favorites: &HashSet<String>| !favorites.is_empty())
        .unwrap_or_else(|| defaults.iter().cloned().collect())
}

fn save_connection_favorites(favorites: &HashSet<String>) -> Result<(), String> {
    let path = connection_favorites_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut names = favorites.iter().cloned().collect::<Vec<_>>();
    names.sort();
    fs::write(path, names.join("\n") + "\n").map_err(|error| error.to_string())
}

fn connection_port(app: &App) -> u16 {
    if app.online {
        app.port
    } else {
        app.cfg.server.port
    }
}

fn connection_model_choices(app: &App) -> Vec<String> {
    let mut choices = Vec::new();
    let mut add = |value: String| {
        if !value.is_empty()
            && value != "no server"
            && value != "no model"
            && !choices.contains(&value)
        {
            choices.push(value);
        }
    };
    add(app.connect_model.clone());
    add(app.model.clone());
    for server in &app.served {
        add(server.model.clone());
    }
    for choice in &app.server.available {
        add(choice.label.clone());
    }
    choices
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(text.to_string()).map_err(|e| e.to_string())
}

// ─────────────────────────── Benchmark ───────────────────────────

#[derive(Clone)]
struct BenchRun {
    pp: f64,
    tg: f64,
    ttft_ms: f64,
    tpot_ms: f64,
    end_to_end_ms: f64,
    output_tokens: u32,
    token_count_source: String,
}

#[derive(Clone)]
struct ConcurrencyRun {
    concurrency: u32,
    completed: u32,
    errors: u32,
    wall_ms: f64,
    system_tokens_per_second: f64,
    mean_request_tokens_per_second: f64,
    p95_latency_ms: f64,
    p95_tpot_ms: f64,
    peak_waiting_requests: Option<u64>,
    peak_kv_cache_usage: Option<f64>,
    peak_server_rss_gib: f64,
    peak_swap_mib: f64,
    min_headroom_gib: f64,
    token_count_source: String,
}

#[derive(Default)]
struct BenchState {
    active: bool,
    in_flight: bool,
    sweep: bool,
    concurrency: bool,
    started_unix: u64,
    label: String,
    task: String,
    prompt_tokens: u32,
    gen_tokens: u32,
    runs: u32,
    sweep_sizes: Vec<u32>,
    concurrency_levels: Vec<u32>,
    run: u32,
    sweep_idx: usize,
    concurrency_idx: usize,
    results: Vec<BenchRun>,
    sweep_results: Vec<(u32, f64)>,
    concurrency_results: Vec<ConcurrencyRun>,
    peak_server_rss_gib: f64,
    peak_swap_mib: f64,
    min_headroom_gib: Option<f64>,
    peak_waiting_requests: Option<u64>,
    peak_kv_cache_usage: Option<f64>,
    point_peak_server_rss_gib: f64,
    point_peak_swap_mib: f64,
    point_min_headroom_gib: Option<f64>,
    point_peak_waiting_requests: Option<u64>,
    point_peak_kv_cache_usage: Option<f64>,
    summary: Option<String>,
}

struct ConcurrentBenchResult {
    concurrency: u32,
    runs: Vec<BenchRun>,
    errors: u32,
    wall_ms: f64,
}

struct BenchResult {
    prompt_tokens: u32,
    gen_tokens: u32,
    run: Option<BenchRun>,
    concurrent: Option<ConcurrentBenchResult>,
}

#[derive(Clone)]
struct BenchmarkRecipe {
    name: String,
    description: String,
    task: String,
    prompt_tokens: u32,
    gen_tokens: u32,
    runs: u32,
    sweep_sizes: Vec<u32>,
    concurrency_levels: Vec<u32>,
}

struct ModelLoadResult {
    label: String,
    result: Result<(), String>,
}

fn benchmark_recipes(app: &App) -> Vec<BenchmarkRecipe> {
    vec![
        BenchmarkRecipe {
            name: "Quick response".into(),
            description: "Configured short deterministic runs for TTFT, TPOT, and decode".into(),
            task: "Count rapidly from 1 to 50.".into(),
            prompt_tokens: app.cfg.benchmark.prompt_tokens,
            gen_tokens: app.cfg.benchmark.gen_tokens,
            runs: app.cfg.benchmark.runs,
            sweep_sizes: Vec::new(),
            concurrency_levels: Vec::new(),
        },
        BenchmarkRecipe {
            name: "Coding turn".into(),
            description: "A repeatable code-shaped prompt for interactive work".into(),
            task: "Write a Python function that checks whether a string is a palindrome.".into(),
            prompt_tokens: 768,
            gen_tokens: 160,
            runs: 3,
            sweep_sizes: Vec::new(),
            concurrency_levels: Vec::new(),
        },
        BenchmarkRecipe {
            name: "Long context".into(),
            description: "Prefill sweep across configured context sizes".into(),
            task: "Summarise the supplied context in five precise bullet points.".into(),
            prompt_tokens: 0,
            gen_tokens: 32,
            runs: 1,
            sweep_sizes: app.cfg.benchmark.sweep.clone(),
            concurrency_levels: Vec::new(),
        },
        BenchmarkRecipe {
            name: "Memory soak".into(),
            description: "Repeated turns to expose allocator growth and swap".into(),
            task: "Give a short, direct answer and do not repeat the question.".into(),
            prompt_tokens: 512,
            gen_tokens: 128,
            runs: 10,
            sweep_sizes: Vec::new(),
            concurrency_levels: Vec::new(),
        },
        BenchmarkRecipe {
            name: "Concurrency sweep".into(),
            description: "1/2/4/8 requests: system rate, p95 latency, queue, and memory".into(),
            task: "Give a short factual answer in one paragraph.".into(),
            prompt_tokens: app.cfg.benchmark.prompt_tokens,
            gen_tokens: app.cfg.benchmark.gen_tokens,
            runs: 1,
            sweep_sizes: Vec::new(),
            concurrency_levels: app.cfg.benchmark.concurrency_levels(),
        },
    ]
}

fn post_benchmark_request(endpoint: &str, payload: &str) -> Result<ureq::Response, Option<u16>> {
    ureq::post(endpoint)
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(180))
        .send_string(payload)
        .map_err(|error| match error {
            ureq::Error::Status(code, _) => Some(code),
            ureq::Error::Transport(_) => None,
        })
}

fn bench_once(
    port: u16,
    model: &str,
    prompt_tokens: u32,
    gen_tokens: u32,
    task: &str,
) -> Option<BenchRun> {
    let filler = "benchmark ".repeat(prompt_tokens as usize / 2);
    let content = format!("{} {}", filler, task);
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": content}],
        "max_tokens": gen_tokens, "temperature": 0, "stream": true,
        "stream_options": {"include_usage": true}
    })
    .to_string();
    let legacy_body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": content}],
        "max_tokens": gen_tokens, "temperature": 0, "stream": true
    })
    .to_string();

    let t0 = Instant::now();
    let mut t_first: Option<Instant> = None;
    let mut t_last = t0;
    let mut tokens = 0u32;
    let mut reported_tokens: Option<u32> = None;
    let mut reported_tg: Option<f64> = None;

    let endpoint = format!("http://127.0.0.1:{}/v1/chat/completions", port);
    let resp = match post_benchmark_request(&endpoint, &body) {
        Ok(response) => response,
        Err(Some(400 | 422)) => post_benchmark_request(&endpoint, &legacy_body).ok()?,
        Err(_) => return None,
    };

    let mut reader = resp.into_reader();
    let mut raw = [0u8; 2048];
    let mut buf = String::new();
    while let Ok(n) = reader.read(&mut raw) {
        if n == 0 {
            break;
        }
        buf.push_str(&String::from_utf8_lossy(&raw[..n]));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf = buf[pos + 1..].to_string();
            if line.starts_with("data: ") && line != "data: [DONE]" {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line[6..]) {
                    reported_tg = v["x_mlx_dspark"]["tokens_per_sec"]
                        .as_f64()
                        .or(v["timings"]["predicted_per_second"].as_f64())
                        .or(reported_tg);
                    reported_tokens = v["usage"]["completion_tokens"]
                        .as_u64()
                        .and_then(|value| u32::try_from(value).ok())
                        .or(reported_tokens);
                    let content = v["choices"][0]["delta"]["content"]
                        .as_str()
                        .or(v["choices"][0]["delta"]["reasoning_content"].as_str())
                        .unwrap_or("");
                    if !content.is_empty() {
                        let now = Instant::now();
                        if t_first.is_none() {
                            t_first = Some(now);
                        }
                        t_last = now;
                        tokens += 1;
                    }
                }
            }
        }
    }

    let tf = t_first?;
    let ttft = (tf - t0).as_secs_f64();
    let gen_dur = (t_last - tf).as_secs_f64();
    let output_tokens = reported_tokens.unwrap_or(tokens);
    if output_tokens < 2 || gen_dur <= 0.0 {
        return None;
    }
    Some(BenchRun {
        pp: prompt_tokens as f64 / ttft.max(0.001),
        // Some runtimes batch committed tokens into one SSE frame. Prefer the
        // server's reported rate and token count; otherwise mark the frame count.
        tg: reported_tg.unwrap_or((output_tokens - 1) as f64 / gen_dur),
        ttft_ms: ttft * 1000.0,
        tpot_ms: gen_dur * 1000.0 / (output_tokens - 1) as f64,
        end_to_end_ms: t0.elapsed().as_secs_f64() * 1000.0,
        output_tokens,
        token_count_source: if reported_tokens.is_some() {
            "server-reported usage".into()
        } else {
            "stream-frame estimate".into()
        },
    })
}

fn bench_concurrent(
    port: u16,
    model: &str,
    prompt_tokens: u32,
    gen_tokens: u32,
    task: &str,
    concurrency: u32,
) -> ConcurrentBenchResult {
    use std::sync::{Arc, Barrier};

    let barrier = Arc::new(Barrier::new(concurrency as usize + 1));
    let mut workers = Vec::with_capacity(concurrency as usize);
    for _ in 0..concurrency {
        let barrier = Arc::clone(&barrier);
        let model = model.to_string();
        let task = task.to_string();
        workers.push(thread::spawn(move || {
            barrier.wait();
            bench_once(port, &model, prompt_tokens, gen_tokens, &task)
        }));
    }
    let started = Instant::now();
    barrier.wait();
    let runs = workers
        .into_iter()
        .filter_map(|worker| worker.join().ok().flatten())
        .collect::<Vec<_>>();
    ConcurrentBenchResult {
        concurrency,
        errors: concurrency.saturating_sub(runs.len() as u32),
        runs,
        wall_ms: started.elapsed().as_secs_f64() * 1000.0,
    }
}

fn mean_std(v: &[f64]) -> (f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0);
    }
    let m = v.iter().sum::<f64>() / v.len() as f64;
    let var = v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64;
    (m, var.sqrt())
}

fn numeric_percentile(values: impl Iterator<Item = f64>, quantile: f64) -> Option<f64> {
    let mut values = values.filter(|value| value.is_finite()).collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    let index = ((values.len() - 1) as f64 * quantile.clamp(0.0, 1.0)).ceil() as usize;
    values.get(index).copied()
}

// ─────────────────────── Interference tracking ───────────────────────

#[derive(Clone)]
struct Offender {
    name: String,
    pid: usize,
    mem_gb: f64,
    cpu: f32,
    hint: &'static str,
}

#[derive(Default)]
struct Interference {
    offenders: Vec<Offender>,
    warnings: Vec<String>,
    selected: usize,
    paused: bool,
    pending_kill: Option<(String, usize, Instant)>,
    cpu_speed_limit: Option<u32>,
    low_power: bool,
    last_slow_check: Option<Instant>,
}

#[cfg(target_os = "macos")]
fn slow_system_checks(inf: &mut Interference) {
    if let Ok(o) = Command::new("pmset").args(["-g", "therm"]).output() {
        let s = String::from_utf8_lossy(&o.stdout);
        let mut min_limit = 100u32;
        for line in s.lines() {
            if line.contains("CPU_Speed_Limit") {
                if let Some(v) = line.split('=').nth(1) {
                    if let Ok(n) = v.trim().parse::<u32>() {
                        min_limit = min_limit.min(n);
                    }
                }
            }
        }
        inf.cpu_speed_limit = Some(min_limit);
    }
    if let Ok(o) = Command::new("pmset").args(["-g"]).output() {
        let s = String::from_utf8_lossy(&o.stdout);
        inf.low_power = s
            .lines()
            .any(|l| l.contains("lowpowermode") && l.trim_end().ends_with('1'));
    }
}

#[cfg(not(target_os = "macos"))]
fn slow_system_checks(_inf: &mut Interference) {}

// ─────────────────────── Context ceiling model ───────────────────────
// Two ceilings: the model's trained window and what memory can physically
// hold. Whichever is lower owns the gauge; the other is a dim footnote.

#[derive(Clone, Copy, PartialEq)]
enum Binding {
    Model,
    Memory,
    Unknown,
}

struct ContextCeiling {
    current_tokens: u64,
    model_max: u32,
    memory_max: u64,
    binding: Binding,
    kv_rate_real: bool,
}

impl ContextCeiling {
    fn effective_max(&self) -> u64 {
        match self.binding {
            Binding::Model => self.model_max as u64,
            Binding::Memory => self.memory_max,
            Binding::Unknown => self.model_max as u64,
        }
    }
}

// ───────────────────────────── App state ─────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Screen {
    Home,
    Measure,
    System,
    Learn,
    Customize,
    Bloat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusPanel {
    HomeModel,
    HomeCapacity,
    HomeSources,
    HomeNext,
    Performance,
    Streams,
    Stages,
    History,
    Memory,
    Pressure,
    Bloat,
    Sources,
}

impl FocusPanel {
    const fn screen(self) -> Screen {
        match self {
            Self::HomeModel | Self::HomeCapacity | Self::HomeSources | Self::HomeNext => {
                Screen::Home
            }
            Self::Performance | Self::Streams | Self::Stages | Self::History => Screen::Measure,
            Self::Memory | Self::Pressure | Self::Bloat | Self::Sources => Screen::System,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExpandedPane {
    Content,
    Guide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelAction {
    Command(CommandAction),
    PreviousRequest,
    NextRequest,
    PreviousProcess,
    NextProcess,
    TogglePressurePause,
    TerminateProcess,
    QuickBloatScan,
    CreateEvalFixture,
}

#[derive(Clone, Copy)]
struct PanelActionItem {
    key: &'static str,
    label: &'static str,
    detail: &'static str,
    action: PanelAction,
    enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ModelTab {
    Local,
    HuggingFace,
    LocalAi,
}

impl ModelTab {
    const fn previous(self) -> Self {
        match self {
            Self::Local => Self::LocalAi,
            Self::HuggingFace => Self::Local,
            Self::LocalAi => Self::HuggingFace,
        }
    }

    const fn next(self) -> Self {
        match self {
            Self::Local => Self::HuggingFace,
            Self::HuggingFace => Self::LocalAi,
            Self::LocalAi => Self::Local,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Popup {
    None,
    Command,
    Models,
    Connect,
    ConnectModels,
    Benchmarks,
    Panels,
    Themes,
    Publish,
}

struct App {
    cfg: Config,
    theme: Theme,
    sys: sysinfo::System,
    device: device::Monitor,
    chip: String,
    total_mem_gb: f64,
    local_ai: local_ai::Source,
    huggingface: huggingface::Catalog,
    agents: agents::Inventory,

    online: bool,
    port: u16,
    ping_ms: f64,
    runtime_observed_at: Option<Instant>,
    served: Vec<ServedModel>,
    model_sources: Vec<ModelSource>,
    engine: String,
    model: String,
    real_vram_gb: Option<f64>,
    real_quant: Option<String>,
    real_params: Option<String>,

    arch: Option<ModelArch>,
    metrics: EngineMetrics,
    latest_round: Option<RoundEvent>,
    runtimes: runtime::Probe,
    real_pp: Option<f64>,
    real_tg: Option<f64>,

    rss_gb: f64,
    cpu_pct: f32,
    host_cpu_pct: f32,
    sys_used_gb: f64,
    swap_mb: f64,
    weights_gb: f64,
    kv_gb: f64,
    headroom_gb: f64,
    ceiling: ContextCeiling,

    bench: BenchState,
    bench_rx: Option<mpsc::Receiver<BenchResult>>,
    model_rx: Option<mpsc::Receiver<ModelLoadResult>>,
    connect_query: String,
    connect_model: String,
    connect_favorites: HashSet<String>,
    spans: VecDeque<RequestSpan>,
    current: Option<RequestSpan>,
    follower: Option<LogFollower>,

    server: ServerManager,
    interference: Interference,
    popup: Popup,
    popup_sel: usize,
    model_tab: ModelTab,
    status_msg: Option<(String, Instant)>,
    screen: Screen,
    panel_sel: usize,
    expanded_panel: Option<FocusPanel>,
    expanded_pane: ExpandedPane,
    expanded_action_sel: usize,
    selected_request_id: Option<String>,
    command_query: String,
    command_sel: usize,
    learn_sel: usize,
    settings_sel: usize,
    theme_choices: Vec<String>,
    theme_query: String,
    bloat: bloat::Scanner,
    bloat_sel: usize,
    bloat_pending_remove: Option<(String, Instant)>,

    tok_hist: VecDeque<f64>,
    prefill_hist: VecDeque<f64>,
    ttft_hist: VecDeque<f64>,
    kv_hist: VecDeque<f64>,
    queue_hist: VecDeque<f64>,
    acceptance_hist: VecDeque<f64>,
    load_hist: VecDeque<f64>,
    dirty: bool,
}

impl App {
    fn new(cfg: Config) -> Self {
        let theme = Theme::load(&cfg.theme);
        let history_samples = cfg.observability.history_samples();
        let theme_choices = theme_choices();
        let follower = LogFollower::new(&cfg.telemetry.log_path);
        let server = ServerManager::new(&cfg.server);
        let runtimes = runtime::Probe::new(cfg.telemetry.ports.clone());
        let models_dir = expand_home(&cfg.server.models_dir);
        let device = device::Monitor::new(&models_dir);
        let huggingface = huggingface::Catalog::new(models_dir);
        let connect_favorites = load_connection_favorites(&cfg.connections.favorites);
        let screen = match cfg.layout.default_view.as_str() {
            "measure" => Screen::Measure,
            "system" => Screen::System,
            "learn" => Screen::Learn,
            "customize" => Screen::Customize,
            "bloat" => Screen::Bloat,
            _ => Screen::Home,
        };
        let bloat_root = {
            let configured = expand_home(&cfg.bloat.project_dir);
            if configured.is_absolute() {
                configured
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(configured)
            }
        };
        let bloat = if cfg.bloat.scan_project {
            bloat::Scanner::new(bloat_root)
        } else {
            bloat::Scanner::runtime_only(bloat_root)
        };
        Self {
            chip: platform::cpu_name(),
            total_mem_gb: 0.0,
            device,
            local_ai: local_ai::Source::new(),
            huggingface,
            agents: agents::Inventory::detect(),
            online: false,
            port: 0,
            ping_ms: 0.0,
            runtime_observed_at: None,
            served: Vec::new(),
            model_sources: Vec::new(),
            engine: "Idle".into(),
            model: "no server".into(),
            real_vram_gb: None,
            real_quant: None,
            real_params: None,
            arch: None,
            metrics: EngineMetrics::default(),
            latest_round: None,
            runtimes,
            real_pp: None,
            real_tg: None,
            rss_gb: 0.0,
            cpu_pct: 0.0,
            host_cpu_pct: 0.0,
            sys_used_gb: 0.0,
            swap_mb: 0.0,
            weights_gb: 0.0,
            kv_gb: 0.0,
            headroom_gb: 0.0,
            ceiling: ContextCeiling {
                current_tokens: 0,
                model_max: 32768,
                memory_max: 0,
                binding: Binding::Unknown,
                kv_rate_real: false,
            },
            bench: BenchState::default(),
            bench_rx: None,
            model_rx: None,
            connect_query: String::new(),
            connect_model: "no server".into(),
            connect_favorites,
            spans: VecDeque::new(),
            current: None,
            follower,
            server,
            interference: Interference::default(),
            popup: Popup::None,
            popup_sel: 0,
            model_tab: ModelTab::Local,
            status_msg: None,
            screen,
            panel_sel: 0,
            expanded_panel: None,
            expanded_pane: ExpandedPane::Content,
            expanded_action_sel: 0,
            selected_request_id: None,
            command_query: String::new(),
            command_sel: 0,
            learn_sel: 0,
            settings_sel: 0,
            theme_choices,
            theme_query: String::new(),
            bloat,
            bloat_sel: 0,
            bloat_pending_remove: None,
            tok_hist: VecDeque::from(vec![0.0; history_samples]),
            prefill_hist: VecDeque::from(vec![0.0; history_samples]),
            ttft_hist: VecDeque::from(vec![0.0; history_samples]),
            kv_hist: VecDeque::from(vec![0.0; history_samples]),
            queue_hist: VecDeque::from(vec![0.0; history_samples]),
            acceptance_hist: VecDeque::from(vec![0.0; history_samples]),
            load_hist: VecDeque::from(vec![0.0; history_samples]),
            dirty: true,
            sys: sysinfo::System::new(),
            cfg,
            theme,
        }
    }

    fn set_status(&mut self, msg: String) {
        self.status_msg = Some((msg, Instant::now()));
        self.dirty = true;
    }

    fn trim_observability_history(&mut self) {
        let limit = self.cfg.observability.history_samples();
        for history in [
            &mut self.tok_hist,
            &mut self.prefill_hist,
            &mut self.ttft_hist,
            &mut self.kv_hist,
            &mut self.queue_hist,
            &mut self.acceptance_hist,
            &mut self.load_hist,
        ] {
            while history.len() > limit {
                history.pop_front();
            }
        }
    }

    fn trim_request_history(&mut self) {
        let limit = self.cfg.observability.request_retention();
        while self.spans.len() > limit {
            self.spans.pop_front();
        }
    }

    fn show_screen(&mut self, screen: Screen) {
        if self.screen != screen {
            self.panel_sel = 0;
        }
        self.screen = screen;
        self.expanded_panel = None;
        self.expanded_pane = ExpandedPane::Content;
        self.expanded_action_sel = 0;
    }

    fn visible_panels(&self) -> Vec<FocusPanel> {
        match self.screen {
            Screen::Home => vec![
                FocusPanel::HomeModel,
                FocusPanel::HomeCapacity,
                FocusPanel::HomeSources,
                FocusPanel::HomeNext,
            ],
            Screen::Measure => [
                ("performance", FocusPanel::Performance),
                ("streams", FocusPanel::Streams),
                ("stages", FocusPanel::Stages),
                ("history", FocusPanel::History),
            ]
            .into_iter()
            .filter_map(|(name, panel)| self.cfg.layout.panel_visible(name).then_some(panel))
            .collect(),
            Screen::System => [
                ("memory", FocusPanel::Memory),
                ("interference", FocusPanel::Pressure),
                ("bloat", FocusPanel::Bloat),
                ("sources", FocusPanel::Sources),
            ]
            .into_iter()
            .filter_map(|(name, panel)| self.cfg.layout.panel_visible(name).then_some(panel))
            .collect(),
            Screen::Learn | Screen::Customize | Screen::Bloat => Vec::new(),
        }
    }

    fn selected_panel(&self) -> Option<FocusPanel> {
        let panels = self.visible_panels();
        panels
            .get(self.panel_sel.min(panels.len().saturating_sub(1)))
            .copied()
    }

    fn selected_panel_position(&self) -> Option<(usize, usize)> {
        let panels = self.visible_panels();
        (!panels.is_empty()).then(|| (self.panel_sel.min(panels.len() - 1) + 1, panels.len()))
    }

    fn panel_actions(&self, panel: FocusPanel) -> Vec<PanelActionItem> {
        let command = |key, label, detail, action| PanelActionItem {
            key,
            label,
            detail,
            action: PanelAction::Command(action),
            enabled: true,
        };
        match panel {
            FocusPanel::HomeModel => vec![
                PanelActionItem {
                    key: "s",
                    label: if self.server.running() {
                        "Stop server"
                    } else {
                        "Serve model"
                    },
                    detail: "start or stop managed local serving",
                    action: PanelAction::Command(CommandAction::Serve),
                    enabled: self.server.running()
                        || self.online
                        || !self.server.available.is_empty(),
                },
                command(
                    "m",
                    "Choose model",
                    "open local targets and model sources",
                    CommandAction::Models,
                ),
                command(
                    "c",
                    "Configure agent",
                    "prepare a detected coding tool",
                    CommandAction::Connect,
                ),
            ],
            FocusPanel::HomeCapacity => vec![
                command(
                    "3",
                    "Inspect system",
                    "open memory and pressure evidence",
                    CommandAction::System,
                ),
                command(
                    "m",
                    "Compare model fit",
                    "review local model footprints",
                    CommandAction::Models,
                ),
                command(
                    "B",
                    "Test context growth",
                    "run the configured prompt sweep",
                    CommandAction::Sweep,
                ),
            ],
            FocusPanel::HomeSources => vec![
                command(
                    "m",
                    "Open inventory",
                    "inspect loaded and installed models",
                    CommandAction::Models,
                ),
                command(
                    "h",
                    "Check Hugging Face",
                    "verify pinned public safetensors",
                    CommandAction::HuggingFace,
                ),
                command(
                    "l",
                    "View sourced comparison",
                    "inspect cached public evidence",
                    CommandAction::LocalAi,
                ),
            ],
            FocusPanel::HomeNext if self.online => vec![
                command(
                    "b",
                    "Run benchmark",
                    "measure the responding model",
                    CommandAction::Benchmark,
                ),
                command(
                    "r",
                    "Choose workload",
                    "select a workload-shaped recipe",
                    CommandAction::Recipes,
                ),
                command(
                    "c",
                    "Configure agent",
                    "connect a detected coding tool",
                    CommandAction::Connect,
                ),
                command(
                    "?",
                    "Explain readings",
                    "open lessons tied to live values",
                    CommandAction::Learn,
                ),
            ],
            FocusPanel::HomeNext => vec![
                command(
                    "m",
                    "Choose a model",
                    "start with a local load target",
                    CommandAction::Models,
                ),
                command(
                    "h",
                    "Check a starter",
                    "verify a small public artifact",
                    CommandAction::HuggingFace,
                ),
                command(
                    "l",
                    "Compare sourced options",
                    "review cached public evidence",
                    CommandAction::LocalAi,
                ),
                command(
                    "c",
                    "Prepare an agent",
                    "generate setup before serving",
                    CommandAction::Connect,
                ),
            ],
            FocusPanel::Performance => vec![
                command(
                    "b",
                    "Run benchmark",
                    "capture a deterministic local baseline",
                    CommandAction::Benchmark,
                ),
                command(
                    "r",
                    "Choose workload",
                    "select chat, code, agent, or context",
                    CommandAction::Recipes,
                ),
                command(
                    "B",
                    "Run prompt sweep",
                    "measure prefill across context sizes",
                    CommandAction::Sweep,
                ),
                command(
                    "?",
                    "Explain metrics",
                    "open live measurement lessons",
                    CommandAction::Learn,
                ),
            ],
            FocusPanel::Streams => vec![
                command(
                    "b",
                    "Generate samples",
                    "run a repeatable short workload",
                    CommandAction::Benchmark,
                ),
                command(
                    "r",
                    "Choose longer workload",
                    "create a more useful session history",
                    CommandAction::Recipes,
                ),
                command(
                    "p",
                    "Preview report",
                    "inspect the checked measurement bundle",
                    CommandAction::Publish,
                ),
            ],
            FocusPanel::Stages => vec![
                PanelActionItem {
                    key: "k",
                    label: "Previous request",
                    detail: "inspect the prior retained trace",
                    action: PanelAction::PreviousRequest,
                    enabled: self.spans.len() > 1,
                },
                PanelActionItem {
                    key: "j",
                    label: "Next request",
                    detail: "inspect the next retained trace",
                    action: PanelAction::NextRequest,
                    enabled: self.spans.len() > 1,
                },
                PanelActionItem {
                    key: "e",
                    label: "Create eval fixture",
                    detail: "save selected metrics as a private human-reviewed fixture",
                    action: PanelAction::CreateEvalFixture,
                    enabled: self.selected_request().is_some(),
                },
                command(
                    "b",
                    "Run request",
                    "add a deterministic request trace",
                    CommandAction::Benchmark,
                ),
            ],
            FocusPanel::History => vec![
                PanelActionItem {
                    key: "k",
                    label: "Previous request",
                    detail: "move selection without losing identity",
                    action: PanelAction::PreviousRequest,
                    enabled: self.spans.len() > 1,
                },
                PanelActionItem {
                    key: "j",
                    label: "Next request",
                    detail: "move selection without losing identity",
                    action: PanelAction::NextRequest,
                    enabled: self.spans.len() > 1,
                },
                PanelActionItem {
                    key: "e",
                    label: "Create eval fixture",
                    detail: "save selected metrics without prompt or response bodies",
                    action: PanelAction::CreateEvalFixture,
                    enabled: self.selected_request().is_some(),
                },
                command(
                    "p",
                    "Preview report",
                    "render redacted Markdown or JSON",
                    CommandAction::Publish,
                ),
                command(
                    "b",
                    "Add benchmark run",
                    "record another local request",
                    CommandAction::Benchmark,
                ),
            ],
            FocusPanel::Memory => vec![
                command(
                    "m",
                    "Compare model footprint",
                    "open local model inventory",
                    CommandAction::Models,
                ),
                command(
                    "B",
                    "Test context growth",
                    "measure memory-bound prompt sizes",
                    CommandAction::Sweep,
                ),
                command(
                    "p",
                    "Preview report",
                    "inspect memory provenance in export",
                    CommandAction::Publish,
                ),
            ],
            FocusPanel::Pressure => vec![
                PanelActionItem {
                    key: "space",
                    label: if self.interference.paused {
                        "Resume process list"
                    } else {
                        "Pause process list"
                    },
                    detail: "freeze or resume identity-stable updates",
                    action: PanelAction::TogglePressurePause,
                    enabled: true,
                },
                PanelActionItem {
                    key: "k",
                    label: "Previous process",
                    detail: "select by stable process identity",
                    action: PanelAction::PreviousProcess,
                    enabled: !self.interference.offenders.is_empty(),
                },
                PanelActionItem {
                    key: "j",
                    label: "Next process",
                    detail: "select by stable process identity",
                    action: PanelAction::NextProcess,
                    enabled: !self.interference.offenders.is_empty(),
                },
                PanelActionItem {
                    key: "x x",
                    label: "Terminate selected",
                    detail: "requires the same action twice",
                    action: PanelAction::TerminateProcess,
                    enabled: !self.interference.offenders.is_empty(),
                },
            ],
            FocusPanel::Bloat => vec![
                command(
                    "6",
                    "Open findings",
                    "inspect selected evidence and guarded cleanup",
                    CommandAction::Bloat,
                ),
                PanelActionItem {
                    key: "g",
                    label: "Run quick scan",
                    detail: "refresh bounded local evidence",
                    action: PanelAction::QuickBloatScan,
                    enabled: !self.bloat.scanning(),
                },
            ],
            FocusPanel::Sources => vec![
                command(
                    "m",
                    "Open model inventory",
                    "inspect local targets and artifacts",
                    CommandAction::Models,
                ),
                command(
                    "c",
                    "Configure agent",
                    "use the selected responding endpoint",
                    CommandAction::Connect,
                ),
                command(
                    "p",
                    "Preview provenance",
                    "inspect the checked public bundle",
                    CommandAction::Publish,
                ),
            ],
        }
    }

    fn create_eval_fixture_from_selected(&mut self) {
        let Some(request) = self.selected_request().cloned() else {
            self.set_status("no request selected for an eval fixture".into());
            return;
        };
        let outcome = match request.stage {
            Stage::Done => "complete",
            Stage::Failed => "failed",
            Stage::Queued => "queued",
            Stage::Prefill => "prefill",
            Stage::Decode => "decode",
        };
        let seed = eval::EvalSeed {
            label: format!("request {}", request.id),
            request_id: request.id.clone(),
            model: public_model_id(&self.model),
            engine: self.engine.clone(),
            prompt_tokens: request.prompt_tokens,
            output_tokens: request.decoded,
            ttft_milliseconds: request.ttft().map(|value| value.as_secs_f64() * 1000.0),
            decode_tokens_per_second: request.decode_rate,
            sampling: request.sampling_summary(),
            outcome: outcome.into(),
        };
        match eval::create_from_measurement(seed) {
            Ok(id) => self.set_status(format!(
                "private eval fixture {id} saved | use tokoro eval create for a content fixture"
            )),
            Err(error) => self.set_status(format!("eval fixture failed: {error}")),
        }
    }

    fn selected_request(&self) -> Option<&RequestSpan> {
        self.selected_request_id
            .as_ref()
            .and_then(|id| {
                self.current
                    .as_ref()
                    .filter(|request| &request.id == id)
                    .or_else(|| self.spans.iter().find(|request| &request.id == id))
            })
            .or(self.current.as_ref())
            .or_else(|| self.spans.back())
    }

    fn select_request(&mut self, backwards: bool) {
        let ids = self
            .current
            .iter()
            .chain(self.spans.iter().rev())
            .map(|request| request.id.clone())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            self.selected_request_id = None;
            return;
        }
        let current = self
            .selected_request_id
            .as_ref()
            .and_then(|selected| ids.iter().position(|id| id == selected))
            .unwrap_or(0);
        let next = if backwards {
            current.checked_sub(1).unwrap_or(ids.len() - 1)
        } else {
            (current + 1) % ids.len()
        };
        self.selected_request_id = Some(ids[next].clone());
    }

    fn select_panel_action(&mut self, backwards: bool) {
        let Some(panel) = self.expanded_panel else {
            return;
        };
        let actions = self.panel_actions(panel);
        if actions.is_empty() {
            self.expanded_action_sel = 0;
            return;
        }
        self.expanded_action_sel = self.expanded_action_sel.min(actions.len() - 1);
        self.expanded_action_sel = if backwards {
            self.expanded_action_sel
                .checked_sub(1)
                .unwrap_or(actions.len() - 1)
        } else {
            (self.expanded_action_sel + 1) % actions.len()
        };
    }

    fn cycle_panel(&mut self, backwards: bool) {
        let panels = self.visible_panels();
        if panels.is_empty() {
            return;
        }
        self.panel_sel = self.panel_sel.min(panels.len() - 1);
        self.panel_sel = if backwards {
            self.panel_sel.checked_sub(1).unwrap_or(panels.len() - 1)
        } else {
            (self.panel_sel + 1) % panels.len()
        };
        if self.expanded_panel.is_some() {
            self.expanded_panel = Some(panels[self.panel_sel]);
            self.expanded_action_sel = 0;
            if matches!(
                panels[self.panel_sel],
                FocusPanel::Stages | FocusPanel::History
            ) {
                self.selected_request_id = self
                    .current
                    .as_ref()
                    .or_else(|| self.spans.back())
                    .map(|request| request.id.clone());
            }
        }
    }

    fn cycle_focus(&mut self, backwards: bool) {
        if self.expanded_panel.is_none() {
            self.cycle_panel(backwards);
            return;
        }
        match (self.expanded_pane, backwards) {
            (ExpandedPane::Content, false) => self.expanded_pane = ExpandedPane::Guide,
            (ExpandedPane::Guide, true) => self.expanded_pane = ExpandedPane::Content,
            (ExpandedPane::Guide, false) => {
                self.cycle_panel(false);
                self.expanded_pane = ExpandedPane::Content;
            }
            (ExpandedPane::Content, true) => {
                self.cycle_panel(true);
                self.expanded_pane = ExpandedPane::Guide;
            }
        }
    }

    fn expand_selected_panel(&mut self) {
        self.expanded_panel = self.selected_panel();
        self.expanded_pane = ExpandedPane::Content;
        self.expanded_action_sel = 0;
        if matches!(
            self.expanded_panel,
            Some(FocusPanel::Stages | FocusPanel::History)
        ) {
            self.selected_request_id = self
                .current
                .as_ref()
                .or_else(|| self.spans.back())
                .map(|request| request.id.clone());
        }
    }

    fn collapse_panel(&mut self) {
        self.expanded_panel = None;
        self.expanded_pane = ExpandedPane::Content;
        self.expanded_action_sel = 0;
    }

    fn kv_rate(&self) -> f64 {
        self.arch
            .map(|a| a.kv_bytes_per_token)
            .unwrap_or_else(|| estimate_kv_rate(&self.model))
    }

    fn check_huggingface(&mut self) {
        let message = if self.huggingface.check() {
            "checking pinned Hugging Face manifests"
        } else {
            "a model check or download is already running"
        };
        self.set_status(message.into());
    }

    fn use_huggingface_selection(&mut self) {
        let Some(entry) = self.huggingface.entries().get(self.popup_sel) else {
            return;
        };
        let repo = entry.repo.clone();
        let label = entry.label.clone();
        if entry.installed() {
            let target =
                huggingface::install_path(&expand_home(&self.cfg.server.models_dir), &repo);
            self.choose_model(ModelChoice {
                target: target.to_string_lossy().into_owned(),
                label,
                detail: "Hugging Face manifest checked; local files".into(),
                can_start: true,
            });
            return;
        }
        let available_bytes = (self.device.storage().available_gib * BYTES_PER_GIB) as u64;
        match self.huggingface.download(self.popup_sel, available_bytes) {
            Ok(true) => self.set_status(format!("downloading {label}")),
            Ok(false) => self.set_status("a model check or download is already running".into()),
            Err(error) => self.set_status(error),
        }
    }

    fn search_huggingface_for_local_ai_selection(&mut self) {
        let Some(reading) = self.local_ai.reading_for(&self.chip, self.total_mem_gb) else {
            self.set_status("no sourced local.ai recommendation is cached".into());
            return;
        };
        let Some(recommendation) = reading.recommendations.get(self.popup_sel) else {
            return;
        };
        let model = recommendation.model.clone();
        match self.huggingface.search(&model) {
            Ok(true) => self.set_status(format!("finding public MLX artifacts for {model}")),
            Ok(false) => self.set_status("a model check or download is already running".into()),
            Err(error) => self.set_status(error),
        }
    }

    fn copy_local_ai_selection(&mut self) {
        let Some(reading) = self.local_ai.reading_for(&self.chip, self.total_mem_gb) else {
            self.set_status("no sourced local.ai recommendation is cached".into());
            return;
        };
        let Some(recommendation) = reading.recommendations.get(self.popup_sel) else {
            return;
        };
        let text = format!(
            "local.ai public recommendation\nMachine: {} / {:.0} GB\nRole: {}\nModel: {}\nIntelligence: {}\nSpeed: {} tasks/hr\nSize: {} GB\nSource: {}",
            reading.machine,
            reading.memory_gb,
            recommendation.label,
            recommendation.model,
            recommendation
                .intelligence
                .map(|value| format!("{value:.1}"))
                .unwrap_or_else(|| "not reported".into()),
            recommendation
                .tasks_per_hour
                .map(|value| format!("{value:.1}"))
                .unwrap_or_else(|| "not reported".into()),
            recommendation
                .size_gb
                .map(|value| format!("{value:.1}"))
                .unwrap_or_else(|| "not reported".into()),
            reading.source_url
        );
        match copy_to_clipboard(&text) {
            Ok(()) => self.set_status("sourced recommendation copied".into()),
            Err(error) => self.set_status(format!("copy failed: {error}")),
        }
    }

    fn refresh_local_ai(&mut self) {
        match self.local_ai.refresh(&self.chip, self.total_mem_gb) {
            Ok(true) => self.set_status(format!(
                "checking public local.ai data for {} / {:.0} GB",
                self.chip, self.total_mem_gb
            )),
            Ok(false) => self.set_status("local.ai refresh already running".into()),
            Err(error) => self.set_status(error),
        }
    }

    fn apply_runtime_snapshot(&mut self, snapshot: runtime::Snapshot, process_engine_found: bool) {
        let old_model = self.model.clone();
        self.runtime_observed_at = Some(Instant::now());
        self.served = snapshot.served;
        self.model_sources = snapshot.model_sources;
        self.metrics = snapshot.metrics;
        self.latest_round = snapshot.latest_round;
        self.real_pp = snapshot.prefill_tokens_per_second;
        self.real_tg = snapshot.decode_tokens_per_second;
        self.real_vram_gb = snapshot.active_model_memory_gib;
        self.real_quant = snapshot.quantization;
        self.real_params = snapshot.parameters;

        if let Some(primary) = snapshot.primary {
            self.online = true;
            self.port = primary.port;
            self.ping_ms = primary.ping_ms;
            self.model = primary.model;
            if !process_engine_found || primary.runtime != "llama.cpp / OpenAI-compatible" {
                self.engine = primary.runtime;
            }
        } else {
            self.online = false;
            self.port = 0;
            self.ping_ms = 0.0;
            self.model = "no model".into();
            self.engine = "Idle".into();
        }

        if self.model != old_model {
            self.arch = None;
            self.ceiling.model_max = 32768;
        }
        if let Some(limit) = snapshot.context_limit {
            self.ceiling.model_max = limit;
        }
        if self.online && self.arch.is_none() {
            self.arch = read_model_arch(&self.cfg.server.models_dir, &self.model);
        }
    }

    fn wait_for_runtime(&mut self, timeout: Duration) {
        if let Some(snapshot) = self.runtimes.wait_latest(timeout) {
            self.apply_runtime_snapshot(snapshot, false);
        }
    }

    fn poll_launch(&mut self) {
        self.runtimes.request();
        if let Some(snapshot) = self.runtimes.take_latest() {
            self.apply_runtime_snapshot(snapshot, false);
        }
        self.server.poll_catalog();
    }

    fn poll(&mut self) -> bool {
        let mut active = false;

        if self
            .device
            .refresh_if_due(&expand_home(&self.cfg.server.models_dir))
        {
            self.dirty = true;
        }
        if let Some(event) = self.local_ai.poll() {
            active = true;
            match event {
                local_ai::Event::Updated(count) => self.set_status(format!(
                    "local.ai updated: {} recommendation{} cached locally",
                    count,
                    if count == 1 { "" } else { "s" }
                )),
                local_ai::Event::Failed(error) => {
                    self.set_status(format!("local.ai refresh failed: {error}"))
                }
            }
        }

        for event in self.huggingface.poll() {
            active = true;
            match event {
                huggingface::Event::Checked(count) => self.set_status(format!(
                    "checked {} Hugging Face manifest{}",
                    count,
                    if count == 1 { "" } else { "s" }
                )),
                huggingface::Event::Searched(count) => {
                    self.model_tab = ModelTab::HuggingFace;
                    self.popup_sel = 0;
                    self.set_status(format!(
                        "found {} checked Hugging Face artifact{}",
                        count,
                        if count == 1 { "" } else { "s" }
                    ));
                }
                huggingface::Event::Downloaded { label, target } => {
                    self.server.add_local_target(&label, &target);
                    self.set_status(format!("downloaded and verified {label}"));
                }
                huggingface::Event::Failed(error) => self.set_status(error),
            }
        }

        if self.server.poll_catalog() {
            active = true;
            self.dirty = true;
        }

        if self.follower.is_none() {
            self.follower = LogFollower::new(&self.cfg.telemetry.log_path);
        }
        if let Some(f) = self.follower.as_mut() {
            if f.poll(&mut self.spans, &mut self.current) {
                active = true;
                self.dirty = true;
            }
        }
        self.trim_request_history();

        self.sys.refresh_memory();
        self.sys.refresh_cpu_usage();
        self.host_cpu_pct = self.sys.global_cpu_usage();
        self.sys
            .refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        self.total_mem_gb = self.sys.total_memory() as f64 / BYTES_PER_GIB;
        self.swap_mb = self.sys.used_swap() as f64 / BYTES_PER_MIB;

        let engine_terms = [
            "mlx-vlm",
            "mlx_lm",
            "mlx-dspark",
            "ollama",
            "llama-server",
            "vllm",
        ];
        let mut found = false;
        let mut engine_count = 0u32;
        let mut offenders: Vec<Offender> = Vec::new();

        for p in self.sys.processes().values() {
            let cmd = p
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            let exe = p.exe();
            let name = exe
                .as_ref()
                .and_then(|e| e.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "?".into());

            let is_engine = engine_terms.iter().any(|t| cmd.contains(t)) && !cmd.contains("tokoro");
            if is_engine {
                engine_count += 1;
                if !found {
                    self.engine = engine_terms
                        .iter()
                        .find(|t| cmd.contains(*t))
                        .unwrap()
                        .to_string();
                    self.rss_gb = p.memory() as f64 / BYTES_PER_GIB;
                    self.cpu_pct = p.cpu_usage();
                    found = true;
                }
                continue;
            }

            let mem_gb = p.memory() as f64 / BYTES_PER_GIB;
            let cpu = p.cpu_usage();
            let denied = DENYLIST.iter().any(|d| name.contains(d) || cmd.contains(d));
            if !denied && (mem_gb > 0.5 || cpu > 25.0) {
                offenders.push(Offender {
                    hint: hint_for(&name),
                    name,
                    pid: p.pid().as_u32() as usize,
                    mem_gb,
                    cpu,
                });
            }
        }
        if !found {
            self.engine = "Idle".into();
            self.rss_gb = 0.0;
            self.cpu_pct = 0.0;
        }
        self.sys_used_gb = (self.sys.used_memory() as f64 / BYTES_PER_GIB - self.rss_gb).max(0.0);
        self.headroom_gb = (self.total_mem_gb - self.rss_gb - self.sys_used_gb).max(0.0);

        offenders.sort_by(|a, b| {
            b.mem_gb
                .partial_cmp(&a.mem_gb)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        offenders.truncate(MAX_OFFENDERS);
        if !self.interference.paused {
            let selected_pid = self
                .interference
                .offenders
                .get(self.interference.selected)
                .map(|offender| offender.pid);
            self.interference.selected = selected_pid
                .and_then(|pid| offenders.iter().position(|offender| offender.pid == pid))
                .unwrap_or(0)
                .min(offenders.len().saturating_sub(1));
            self.interference.offenders = offenders;

            let mut warnings = Vec::new();
            if self.swap_mb > 500.0 {
                warnings.push(format!(
                    "swap {:.1} GiB active - decode may stutter; reduce memory pressure",
                    self.swap_mb / 1024.0
                ));
            }
            if engine_count > 1 {
                warnings.push(format!(
                    "{} inference servers loaded - duplicate weights in RAM",
                    engine_count
                ));
            }
            if self.headroom_gb < 10.0 && self.total_mem_gb > 0.0 {
                warnings.push("low headroom - context may spill to swap".into());
            }
            if let Some(limit) = self.interference.cpu_speed_limit {
                if limit < 100 {
                    warnings.push(format!(
                        "thermal throttle - clocks at {}% (prefill may slow)",
                        limit
                    ));
                }
            }
            if self.interference.low_power {
                warnings.push("low power mode on - GPU clocks capped".into());
            }
            self.interference.warnings = warnings;

            let due = self
                .interference
                .last_slow_check
                .map(|t| t.elapsed() > Duration::from_secs(10))
                .unwrap_or(true);
            if due {
                slow_system_checks(&mut self.interference);
                self.interference.last_slow_check = Some(Instant::now());
            }
        }

        // Runtime adapters probe on a single-flight worker. Slow localhost ports
        // never block input, resize, or drawing on the terminal thread.
        self.runtimes.request();
        if let Some(snapshot) = self.runtimes.take_latest() {
            active = true;
            self.apply_runtime_snapshot(snapshot, found);
        }

        let kv_rate = self.kv_rate();

        // Memory stack
        let weights = self
            .real_vram_gb
            .unwrap_or_else(|| estimate_weights(&self.model).min(self.rss_gb));
        self.weights_gb = weights;
        self.kv_gb = (self.rss_gb - weights).max(0.0) * 0.85;

        // Two-ceiling context model
        if let Some(a) = self.arch {
            if self.ceiling.model_max == 32768 {
                // not overridden by server
                self.ceiling.model_max = a.max_context;
            }
        }
        self.ceiling.memory_max = (self.headroom_gb * 1024.0 / kv_rate.max(0.05)) as u64;
        self.ceiling.kv_rate_real = self.arch.is_some();
        self.ceiling.current_tokens = self
            .metrics
            .kv_cache_tokens
            .unwrap_or_else(|| (self.kv_gb * 1024.0 / kv_rate.max(0.05)) as u64);
        self.ceiling.binding = if self.ceiling.model_max as u64 <= self.ceiling.memory_max {
            Binding::Model
        } else {
            Binding::Memory
        };

        let history_limit = self.cfg.observability.history_samples();
        let decode = self
            .real_tg
            .or(self.current.as_ref().map(|c| c.decode_rate))
            .unwrap_or(0.0);
        push_ring(&mut self.tok_hist, decode, history_limit);
        let prefill = self
            .real_pp
            .or(self.current.as_ref().map(|request| request.prefill_rate))
            .unwrap_or(0.0);
        push_ring(&mut self.prefill_hist, prefill, history_limit);
        let ttft_ms = self
            .current
            .as_ref()
            .and_then(RequestSpan::ttft)
            .or_else(|| self.spans.back().and_then(RequestSpan::ttft))
            .map(|duration| duration.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        push_ring(&mut self.ttft_hist, ttft_ms, history_limit);
        let kv_usage = self
            .metrics
            .kv_cache_usage
            .map(|usage| usage * 100.0)
            .unwrap_or_else(|| {
                self.ceiling.current_tokens as f64 / self.ceiling.effective_max().max(1) as f64
                    * 100.0
            });
        push_ring(&mut self.kv_hist, kv_usage, history_limit);
        push_ring(
            &mut self.queue_hist,
            self.metrics.requests_waiting.unwrap_or(0) as f64,
            history_limit,
        );
        push_ring(
            &mut self.acceptance_hist,
            self.metrics.draft_acceptance.unwrap_or(0.0) * 100.0,
            history_limit,
        );
        let load = if self.rss_gb > 0.0 {
            (self.cpu_pct as f64).min(100.0)
        } else {
            0.0
        };
        push_ring(&mut self.load_hist, load, history_limit);

        self.bloat.update_runtime(bloat::RuntimeSnapshot {
            loaded_endpoints: self
                .served
                .iter()
                .filter(|server| server.state == "loaded")
                .count(),
            swap_mib: self.swap_mb,
            prompt_tokens: self
                .current
                .as_ref()
                .map(|request| request.prompt_tokens)
                .unwrap_or(0),
            prefix_hits: self.metrics.prefix_hits.unwrap_or(0),
            prefix_partial_hits: self.metrics.prefix_partial_hits.unwrap_or(0),
            probe_ports: self.cfg.telemetry.ports.len(),
        });
        for event in self.bloat.poll() {
            active = true;
            match event {
                bloat::ScannerEvent::ScanCompleted { findings } => {
                    if self.screen == Screen::Bloat {
                        self.set_status(format!(
                            "quick scan complete: {} project findings",
                            findings
                        ));
                    }
                }
                bloat::ScannerEvent::RemovalCompleted(result) => {
                    self.bloat_pending_remove = None;
                    match result {
                        Ok(message) => self.set_status(message),
                        Err(error) => self.set_status(format!("removal refused: {}", error)),
                    }
                }
            }
        }
        let finding_count = self.bloat.findings().len();
        if finding_count == 0 {
            self.bloat_sel = 0;
        } else {
            self.bloat_sel = self.bloat_sel.min(finding_count - 1);
        }

        if let Some((_, t0)) = &self.status_msg {
            if t0.elapsed() > Duration::from_secs(4) {
                self.status_msg = None;
                self.dirty = true;
            }
        }

        // Polling updates are the live view. Redraw even when the log file is quiet so
        // endpoint state, memory, and allocator metrics do not look frozen.
        self.dirty = true;
        active
    }

    fn try_kill_selected(&mut self) {
        let Some(o) = self.interference.offenders.get(self.interference.selected) else {
            return;
        };
        let (name, pid) = (o.name.clone(), o.pid);

        match &self.interference.pending_kill {
            Some((n, p, t)) if *n == name && *p == pid && t.elapsed() < Duration::from_secs(3) => {
                let pid_t = sysinfo::Pid::from(pid);
                let ok = self
                    .sys
                    .process(pid_t)
                    .map(|pr| {
                        pr.kill_with(sysinfo::Signal::Term)
                            .unwrap_or_else(|| pr.kill())
                    })
                    .unwrap_or(false);
                self.interference.pending_kill = None;
                self.set_status(if ok {
                    format!("terminated {}", name)
                } else {
                    format!("failed to kill {}", name)
                });
            }
            _ => {
                self.interference.pending_kill = Some((name.clone(), pid, Instant::now()));
                self.set_status(format!("press x again to kill {} (pid {})", name, pid));
            }
        }
    }

    fn try_remove_selected_bloat(&mut self) {
        let findings = self.bloat.findings();
        let Some(finding) = findings.get(self.bloat_sel).cloned() else {
            return;
        };
        if !finding.can_remove() {
            self.set_status("review findings are never auto-deleted".into());
            return;
        }
        let identity = finding
            .relative_path()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| finding.code.into());
        let confirmed = self
            .bloat_pending_remove
            .as_ref()
            .is_some_and(|(pending, at)| {
                pending == &identity && at.elapsed() < Duration::from_secs(4)
            });
        if confirmed {
            self.bloat_pending_remove = None;
            match self.bloat.remove(&finding) {
                Ok(()) => self.set_status(format!("removing {}", identity)),
                Err(error) => self.set_status(format!("removal refused: {}", error)),
            }
        } else {
            self.bloat_pending_remove = Some((identity.clone(), Instant::now()));
            self.set_status(format!("press d again to remove {}", identity));
        }
    }

    fn choose_model(&mut self, choice: ModelChoice) {
        if !choice.can_start {
            self.set_status(format!("not started: {} does not fit RAM", choice.label));
            return;
        }
        let target = choice.target.clone();
        if self.online {
            if self.model_rx.is_some() {
                self.set_status("model load already in progress".into());
                return;
            }
            let port = self.port;
            let label = choice.label.clone();
            let (tx, rx) = mpsc::channel();
            self.model_rx = Some(rx);
            self.set_status(format!("loading {} | {}", choice.label, choice.detail));
            thread::spawn(move || {
                let result = runtime::post_model_load(port, &target);
                let _ = tx.send(ModelLoadResult { label, result });
            });
            return;
        }
        if self.server.running() {
            self.server.stop();
        }
        match self
            .server
            .start(&self.cfg.server, &target, &self.cfg.telemetry.log_path)
        {
            Ok(()) => self.set_status(format!("starting {} | {}", choice.label, choice.detail)),
            Err(error) => self.set_status(format!("start failed: {}", error)),
        }
    }

    fn poll_model_load(&mut self) {
        let result = match self.model_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(result)) => Some(result),
            Some(Err(mpsc::TryRecvError::Disconnected)) => Some(ModelLoadResult {
                label: "model".into(),
                result: Err("load worker stopped".into()),
            }),
            _ => None,
        };
        if let Some(result) = result {
            self.model_rx = None;
            match result.result {
                Ok(()) => self.set_status(format!("model load complete: {}", result.label)),
                Err(error) => self.set_status(format!("model load failed: {}", error)),
            }
        }
    }

    fn start_benchmark(&mut self, sweep: bool) {
        let recipe = if sweep {
            BenchmarkRecipe {
                name: "prompt sweep".into(),
                description: "configured prompt-length sweep".into(),
                task: "Explain the result clearly and concisely.".into(),
                prompt_tokens: 0,
                gen_tokens: 32,
                runs: 1,
                sweep_sizes: self.cfg.benchmark.sweep.clone(),
                concurrency_levels: Vec::new(),
            }
        } else {
            BenchmarkRecipe {
                name: "quick response".into(),
                description: "configured short deterministic runs".into(),
                task: "Count rapidly from 1 to 50.".into(),
                prompt_tokens: self.cfg.benchmark.prompt_tokens,
                gen_tokens: self.cfg.benchmark.gen_tokens,
                runs: self.cfg.benchmark.runs,
                sweep_sizes: Vec::new(),
                concurrency_levels: Vec::new(),
            }
        };
        self.start_benchmark_plan(recipe);
    }

    fn start_benchmark_plan(&mut self, recipe: BenchmarkRecipe) {
        if !self.online || self.bench.active {
            return;
        }
        self.bench = BenchState {
            active: true,
            sweep: !recipe.sweep_sizes.is_empty(),
            concurrency: !recipe.concurrency_levels.is_empty(),
            started_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
            label: recipe.name,
            task: recipe.task,
            prompt_tokens: recipe.prompt_tokens,
            gen_tokens: recipe.gen_tokens,
            runs: recipe.runs,
            sweep_sizes: recipe.sweep_sizes,
            concurrency_levels: recipe.concurrency_levels,
            ..Default::default()
        };
        self.bench_rx = None;
        self.dirty = true;
    }

    fn bench_tick(&mut self) {
        if !self.bench.active || !self.online {
            return;
        }
        self.observe_benchmark_pressure();

        if self.bench.in_flight {
            let result = match self.bench_rx.as_ref().map(|rx| rx.try_recv()) {
                Some(Ok(result)) => Some(result),
                Some(Err(mpsc::TryRecvError::Disconnected)) => Some(BenchResult {
                    prompt_tokens: 0,
                    gen_tokens: 0,
                    run: None,
                    concurrent: None,
                }),
                _ => None,
            };
            if let Some(result) = result {
                self.bench_rx = None;
                self.bench.in_flight = false;
                self.finish_benchmark(result);
            }
            return;
        }

        let (prompt_tokens, gen_tokens) = if self.bench.sweep {
            (
                self.bench
                    .sweep_sizes
                    .get(self.bench.sweep_idx)
                    .copied()
                    .unwrap_or(512),
                self.bench.gen_tokens,
            )
        } else {
            (self.bench.prompt_tokens, self.bench.gen_tokens)
        };
        let concurrency = self.bench.concurrency.then(|| {
            self.bench
                .concurrency_levels
                .get(self.bench.concurrency_idx)
                .copied()
                .unwrap_or(1)
        });
        if concurrency.is_some() {
            self.reset_benchmark_point_pressure();
            self.observe_benchmark_pressure();
        }
        let port = self.port;
        let model = self.model.clone();
        let task = self.bench.task.clone();
        let (tx, rx) = mpsc::channel();
        self.bench_rx = Some(rx);
        self.bench.in_flight = true;
        thread::spawn(move || {
            let result = if let Some(concurrency) = concurrency {
                BenchResult {
                    prompt_tokens,
                    gen_tokens,
                    run: None,
                    concurrent: Some(bench_concurrent(
                        port,
                        &model,
                        prompt_tokens,
                        gen_tokens,
                        &task,
                        concurrency,
                    )),
                }
            } else {
                BenchResult {
                    prompt_tokens,
                    gen_tokens,
                    run: bench_once(port, &model, prompt_tokens, gen_tokens, &task),
                    concurrent: None,
                }
            };
            let _ = tx.send(result);
        });
        self.dirty = true;
    }

    fn observe_benchmark_pressure(&mut self) {
        let waiting = self.metrics.requests_waiting;
        let kv = self.metrics.kv_cache_usage;
        self.bench.peak_server_rss_gib = self.bench.peak_server_rss_gib.max(self.rss_gb);
        self.bench.peak_swap_mib = self.bench.peak_swap_mib.max(self.swap_mb);
        self.bench.min_headroom_gib = Some(
            self.bench
                .min_headroom_gib
                .map_or(self.headroom_gb, |value| value.min(self.headroom_gb)),
        );
        self.bench.peak_waiting_requests = match (self.bench.peak_waiting_requests, waiting) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (None, value) | (value, None) => value,
        };
        self.bench.peak_kv_cache_usage = match (self.bench.peak_kv_cache_usage, kv) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (None, value) | (value, None) => value,
        };
        self.bench.point_peak_server_rss_gib =
            self.bench.point_peak_server_rss_gib.max(self.rss_gb);
        self.bench.point_peak_swap_mib = self.bench.point_peak_swap_mib.max(self.swap_mb);
        self.bench.point_min_headroom_gib = Some(
            self.bench
                .point_min_headroom_gib
                .map_or(self.headroom_gb, |value| value.min(self.headroom_gb)),
        );
        self.bench.point_peak_waiting_requests =
            match (self.bench.point_peak_waiting_requests, waiting) {
                (Some(left), Some(right)) => Some(left.max(right)),
                (None, value) | (value, None) => value,
            };
        self.bench.point_peak_kv_cache_usage = match (self.bench.point_peak_kv_cache_usage, kv) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (None, value) | (value, None) => value,
        };
    }

    fn reset_benchmark_point_pressure(&mut self) {
        self.bench.point_peak_server_rss_gib = 0.0;
        self.bench.point_peak_swap_mib = 0.0;
        self.bench.point_min_headroom_gib = None;
        self.bench.point_peak_waiting_requests = None;
        self.bench.point_peak_kv_cache_usage = None;
    }

    fn push_benchmark_span(
        &mut self,
        id: String,
        prompt_tokens: u32,
        gen_tokens: u32,
        run: &BenchRun,
    ) {
        let mut span = RequestSpan::new(id);
        span.prompt_tokens = prompt_tokens;
        span.prefill_done = prompt_tokens;
        span.prefill_rate = run.pp;
        span.decoded = run.output_tokens;
        span.decode_rate = run.tg;
        span.max_tokens = Some(gen_tokens);
        span.temperature = Some(0.0);
        span.stage = Stage::Done;
        span.first_token = span
            .started
            .checked_add(Duration::from_secs_f64(run.ttft_ms / 1000.0));
        span.last_update = span
            .started
            .checked_add(Duration::from_secs_f64(run.end_to_end_ms / 1000.0))
            .unwrap_or_else(Instant::now);
        if self.follower.is_none() {
            self.spans.push_back(span);
            self.trim_request_history();
        }
    }

    fn finish_benchmark(&mut self, result: BenchResult) {
        if let Some(concurrent) = result.concurrent {
            let completed = concurrent.runs.len() as u32;
            let total_tokens = concurrent
                .runs
                .iter()
                .map(|run| run.output_tokens as u64)
                .sum::<u64>();
            let system_tokens_per_second =
                total_tokens as f64 / (concurrent.wall_ms / 1000.0).max(0.001);
            let mean_request_tokens_per_second = if concurrent.runs.is_empty() {
                0.0
            } else {
                concurrent.runs.iter().map(|run| run.tg).sum::<f64>() / concurrent.runs.len() as f64
            };
            let p95_latency_ms =
                numeric_percentile(concurrent.runs.iter().map(|run| run.end_to_end_ms), 0.95)
                    .unwrap_or(0.0);
            let p95_tpot_ms =
                numeric_percentile(concurrent.runs.iter().map(|run| run.tpot_ms), 0.95)
                    .unwrap_or(0.0);
            let token_count_source = if concurrent
                .runs
                .iter()
                .all(|run| run.token_count_source == "server-reported usage")
            {
                "server-reported usage"
            } else {
                "includes stream-frame estimates"
            }
            .to_string();
            for (index, run) in concurrent.runs.iter().enumerate() {
                self.push_benchmark_span(
                    format!("conc-{}-{}", concurrent.concurrency, index + 1),
                    result.prompt_tokens,
                    result.gen_tokens,
                    run,
                );
            }
            self.bench.concurrency_results.push(ConcurrencyRun {
                concurrency: concurrent.concurrency,
                completed,
                errors: concurrent.errors,
                wall_ms: concurrent.wall_ms,
                system_tokens_per_second,
                mean_request_tokens_per_second,
                p95_latency_ms,
                p95_tpot_ms,
                peak_waiting_requests: self.bench.point_peak_waiting_requests,
                peak_kv_cache_usage: self.bench.point_peak_kv_cache_usage,
                peak_server_rss_gib: self.bench.point_peak_server_rss_gib,
                peak_swap_mib: self.bench.point_peak_swap_mib,
                min_headroom_gib: self.bench.point_min_headroom_gib.unwrap_or(0.0),
                token_count_source,
            });
            self.bench.concurrency_idx += 1;
            if self.bench.concurrency_idx >= self.bench.concurrency_levels.len() {
                let last = self.bench.concurrency_results.last();
                self.bench.summary = Some(match last {
                    Some(last) => format!(
                        "{} | c{} {:.1} system tok/s | p95 {:.0}ms | {} errors",
                        self.bench.label,
                        last.concurrency,
                        last.system_tokens_per_second,
                        last.p95_latency_ms,
                        last.errors
                    ),
                    None => "concurrency benchmark failed".into(),
                });
                self.bench.active = false;
            }
            self.dirty = true;
            return;
        }

        let Some(run) = result.run else {
            self.bench.summary = Some("benchmark failed - server unreachable".into());
            self.bench.active = false;
            self.dirty = true;
            return;
        };

        self.push_benchmark_span(
            format!("bench-{}", self.bench.run + 1),
            result.prompt_tokens,
            result.gen_tokens,
            &run,
        );

        if self.bench.sweep {
            self.bench
                .sweep_results
                .push((result.prompt_tokens, run.pp));
            self.bench.sweep_idx += 1;
            if self.bench.sweep_idx >= self.bench.sweep_sizes.len() {
                let summary = self
                    .bench
                    .sweep_results
                    .iter()
                    .map(|(tokens, pp)| format!("{}k->{:.0}", tokens / 1024, pp))
                    .collect::<Vec<_>>()
                    .join("  ");
                self.bench.summary = Some(format!("{} | {} tok/s", self.bench.label, summary));
                self.bench.active = false;
            }
        } else {
            self.bench.results.push(run);
            self.bench.run += 1;
            if self.bench.run >= self.bench.runs {
                let pps: Vec<f64> = self.bench.results.iter().map(|r| r.pp).collect();
                let tgs: Vec<f64> = self.bench.results.iter().map(|r| r.tg).collect();
                let ttfts: Vec<f64> = self.bench.results.iter().map(|r| r.ttft_ms).collect();
                let (pp_m, pp_s) = mean_std(&pps);
                let (tg_m, tg_s) = mean_std(&tgs);
                let (tt_m, _) = mean_std(&ttfts);
                self.bench.summary = Some(format!(
                    "{} | pp{:.0}+/-{:.0} tok/s | tg{:.1}+/-{:.1} tok/s | ttft {:.0}ms | {} runs",
                    self.bench.label, pp_m, pp_s, tg_m, tg_s, tt_m, self.bench.runs
                ));
                self.bench.active = false;
            }
        }
        self.dirty = true;
    }
}

fn push_ring(d: &mut VecDeque<f64>, v: f64, limit: usize) {
    d.push_back(v);
    while d.len() > limit.max(1) {
        d.pop_front();
    }
}

fn public_model_id(model: &str) -> String {
    if model.starts_with('/') || model.starts_with("~/") || model.contains('\\') {
        PathBuf::from(model)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "local-model".into())
    } else {
        model.to_string()
    }
}

fn infer_parameter_billions(model: &str) -> Option<f64> {
    model
        .to_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.')
        .find_map(|token| {
            token
                .strip_suffix('b')
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| (1.0..=1000.0).contains(value))
        })
}

fn infer_quantization(model: &str) -> Option<String> {
    let lower = model.to_lowercase();
    for (needle, label) in [
        ("16-bit", "16-bit"),
        ("16bit", "16-bit"),
        ("bf16", "BF16"),
        ("fp16", "FP16"),
        ("8-bit", "8-bit"),
        ("8bit", "8-bit"),
        ("q8", "Q8"),
        ("4-bit", "4-bit"),
        ("4bit", "4-bit"),
        ("q4", "Q4"),
        ("2-bit", "2-bit"),
        ("2bit", "2-bit"),
        ("q2", "Q2"),
    ] {
        if lower.contains(needle) {
            return Some(label.into());
        }
    }
    None
}

fn estimate_weights(model: &str) -> f64 {
    let m = model.to_lowercase();
    let params = if m.contains("70b") {
        70.0
    } else if m.contains("27b") {
        27.0
    } else if m.contains("8b") {
        8.0
    } else {
        7.0
    };
    let bpp = if m.contains("4bit") || m.contains("q4") {
        0.55
    } else if m.contains("6bit") || m.contains("q6") {
        0.8
    } else if m.contains("fp16") || m.contains("bf16") {
        2.0
    } else {
        1.05
    };
    params * bpp
}

fn estimate_kv_rate(model: &str) -> f64 {
    let m = model.to_lowercase();
    let params = if m.contains("70b") {
        70.0
    } else if m.contains("27b") {
        27.0
    } else if m.contains("8b") {
        8.0
    } else {
        7.0
    };
    if m.contains("deepseek") {
        0.12
    } else if m.contains("llama") {
        (params / 8.0) * 0.13
    } else {
        (params / 27.0) * 0.13
    }
}

// ─────────────────────────────── main ───────────────────────────────

enum InputEvent {
    Key(KeyEvent),
    Resize,
}

struct ResizeDebouncer {
    quiet_period: Duration,
    last_event: Option<Instant>,
}

impl Default for ResizeDebouncer {
    fn default() -> Self {
        Self {
            quiet_period: Duration::from_millis(100),
            last_event: None,
        }
    }
}

impl ResizeDebouncer {
    fn notify(&mut self, now: Instant) {
        self.last_event = Some(now);
    }

    fn pending(&self) -> bool {
        self.last_event.is_some()
    }

    fn settled(&mut self, now: Instant) -> bool {
        let Some(last) = self.last_event else {
            return false;
        };
        if now.duration_since(last) < self.quiet_period {
            return false;
        }
        self.last_event = None;
        true
    }
}

pub fn run() -> io::Result<()> {
    if cli::run_if_requested().map_err(io::Error::other)? {
        return Ok(());
    }
    let cfg = load_config();
    let intro_config = cfg.intro.clone();
    let mut launch_intro =
        intro::should_run(&intro_config).then(|| intro::Session::new(&intro_config));
    let mut app = App::new(cfg);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;
    if launch_intro.is_some() {
        intro::play_sound(&intro_config);
    }

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || loop {
        if let Ok(true) = event::poll(Duration::from_millis(100)) {
            let Ok(event) = event::read() else { continue };
            let input = match event {
                Event::Key(k) => InputEvent::Key(k),
                Event::Resize(_, _) => InputEvent::Resize,
                _ => continue,
            };
            if tx.send(input).is_err() {
                break;
            }
        }
    });

    let mut next_tick = Instant::now();
    let mut resize = ResizeDebouncer::default();
    let mut last_intro_frame = None;
    loop {
        while let Ok(input) = rx.try_recv() {
            if matches!(input, InputEvent::Resize) {
                // Terminal emulators emit one resize event per drag step. Keep the
                // last frame on screen and redraw once the size has been quiet.
                resize.notify(Instant::now());
                app.dirty = true;
                continue;
            }
            let InputEvent::Key(k) = input else { continue };
            app.dirty = true;
            if launch_intro.take().is_some() {
                last_intro_frame = None;
                continue;
            }
            if matches!(app.handle_key(k), input::InputControl::Quit) {
                app.server.stop();
                disable_raw_mode()?;
                execute!(term.backend_mut(), LeaveAlternateScreen)?;
                term.show_cursor()?;
                return Ok(());
            }
        }

        app.poll_model_load();
        if app.bench.active {
            app.bench_tick();
        }

        if resize.settled(Instant::now()) {
            app.dirty = true;
        }

        let now = Instant::now();
        if launch_intro
            .as_ref()
            .is_some_and(|session| session.finished(now))
        {
            launch_intro = None;
            last_intro_frame = None;
            next_tick = now;
            app.dirty = true;
        } else if let Some(session) = &launch_intro {
            let frame_key = session.frame_key(now);
            if last_intro_frame != Some(frame_key) {
                last_intro_frame = Some(frame_key);
                app.dirty = true;
            }
        }

        if Instant::now() >= next_tick {
            let ms = if launch_intro.is_some() {
                app.poll_launch();
                100
            } else if app.poll() {
                app.cfg.telemetry.active_ms
            } else {
                app.cfg.telemetry.idle_ms
            };
            next_tick = Instant::now() + Duration::from_millis(ms);
        }

        if app.dirty && !resize.pending() {
            // Terminal::draw calls ratatui's autoresize. Do not clear the screen
            // here: a clear for every resize event is the source of visible flicker.
            if let Some(session) = &launch_intro {
                term.draw(|frame| session.render(frame, &app.theme))?;
            } else {
                term.draw(|frame| ui::render(frame, &app))?;
            }
            app.dirty = false;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn rendered_text(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| ui::render(frame, app))
            .expect("screen should render");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn last_used_row(text: &str, width: usize) -> usize {
        let cells = text.chars().collect::<Vec<_>>();
        cells
            .chunks(width)
            .enumerate()
            .filter(|(_, row)| row.iter().any(|cell| !cell.is_whitespace()))
            .map(|(index, _)| index)
            .next_back()
            .unwrap_or(0)
    }

    #[test]
    fn follows_live_decode_progress() {
        let mut spans = VecDeque::new();
        let mut current = None;
        LogFollower::parse_line(
            "Generation queued: request=abc prompt_tokens=512 max_tokens=128 temperature=0.20 top_p=0.95 top_k=40",
            &mut spans,
            &mut current,
        );
        LogFollower::parse_line(
            "Decode started: request=abc time_to_first_token=0.42s",
            &mut spans,
            &mut current,
        );
        LogFollower::parse_line(
            "Decode progress: request=abc generated_tokens=40 elapsed=2.0s rate=20.0 tok/s",
            &mut spans,
            &mut current,
        );
        let request = current.expect("request should remain live");
        assert_eq!(request.stage, Stage::Decode);
        assert_eq!(request.decoded, 40);
        assert_eq!(request.decode_rate, 20.0);
        assert_eq!(request.max_tokens, Some(128));
        assert_eq!(request.temperature, Some(0.2));
        assert_eq!(request.top_p, Some(0.95));
        assert_eq!(request.top_k, Some(40));
        assert!(request.sampling_summary().contains("top-k 40"));
    }

    #[test]
    fn server_argument_templates_keep_windows_paths_as_one_argument() {
        let arguments = server_arguments(
            "--model {model} --port {port} --label \"local model\"",
            r"C:\Models\Qwen 7B\model.gguf",
            8080,
        )
        .expect("valid argument template");
        assert_eq!(
            arguments,
            vec![
                "--model",
                r"C:\Models\Qwen 7B\model.gguf",
                "--port",
                "8080",
                "--label",
                "local model",
            ]
        );
        assert_eq!(
            server_arguments(
                r#"--cache "C:\Program Files\Tokoro Cache""#,
                "model.gguf",
                8080,
            )
            .expect("Windows literal path"),
            vec!["--cache", r"C:\Program Files\Tokoro Cache"]
        );
    }

    #[test]
    fn adaptive_dashboards_give_growing_lists_the_extra_height() {
        fn row_of(text: &str, width: usize, needle: &str) -> usize {
            text.chars()
                .collect::<Vec<_>>()
                .chunks(width)
                .position(|row| row.iter().collect::<String>().contains(needle))
                .expect("panel title should render")
        }

        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);

        let home = rendered_text(&app, 120, 36);
        assert!(row_of(&home, 120, "INVENTORY") < 18);
        assert_ne!(row_of(&home, 120, "INVENTORY"), row_of(&home, 120, "NEXT"));
        assert!(home.contains("LOCAL MODEL PATH"));
        assert!(!home.contains('█'));

        app.show_screen(Screen::Measure);
        let measure = rendered_text(&app, 120, 36);
        assert!(row_of(&measure, 120, "REQUEST HISTORY") > row_of(&measure, 120, "INFERENCE PATH"));
        assert!(measure.contains("independent scales"));

        app.show_screen(Screen::System);
        let system = rendered_text(&app, 120, 36);
        assert!(
            row_of(&system, 120, "ENDPOINTS / PROVENANCE") > row_of(&system, 120, "MEMORY STACK")
        );
        assert!(system.contains("HOST"));
    }

    #[test]
    fn prefers_descriptive_qwen_alias_for_auto_dflash() {
        let aliases = ["qwen38", "qwen3.8-27b-uncensored-8bit"];
        let selected = aliases.iter().min_by_key(|model| model_priority(model));
        assert_eq!(selected, Some(&"qwen3.8-27b-uncensored-8bit"));
    }

    #[test]
    fn labels_precision_directories_with_the_model_family() {
        assert_eq!(
            model_display_label("/models/qwen3.8-27b-unc/8-bit"),
            "qwen3.8-27b-unc/8-bit"
        );
        assert_eq!(model_display_label("llama3.2:3b"), "llama3.2:3b");
    }

    #[test]
    fn resize_debouncer_waits_for_quiet_terminal() {
        let now = Instant::now();
        let mut debouncer = ResizeDebouncer::default();
        debouncer.notify(now);
        assert!(debouncer.pending());
        assert!(!debouncer.settled(now + Duration::from_millis(99)));
        assert!(debouncer.settled(now + Duration::from_millis(100)));
        assert!(!debouncer.pending());
    }

    #[test]
    fn renders_common_terminal_sizes() {
        let mut app = App::new(Config::default());
        for (width, height) in [(80, 24), (100, 32), (140, 40)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| ui::render(frame, &app))
                .expect("dashboard should render");
            let text: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(text.contains("NO MODEL LOADED"));
            assert!(text.contains("disk free"));
            assert!(text.contains("RAM"));
            assert!(!text.contains("KEYS:"));
            app.dirty = true;
        }
    }

    #[test]
    fn every_screen_uses_small_tall_normal_and_wide_viewports() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        for screen in [
            Screen::Home,
            Screen::Measure,
            Screen::System,
            Screen::Learn,
            Screen::Customize,
            Screen::Bloat,
        ] {
            app.screen = screen;
            for (width, height) in [(64, 16), (64, 32), (80, 24), (100, 32), (140, 40)] {
                let text = rendered_text(&app, width, height);
                assert!(
                    last_used_row(&text, width as usize) >= height as usize - 2,
                    "{screen:?} leaves the bottom of {width}x{height} unused"
                );
            }
        }
    }

    #[test]
    fn normal_terminal_dashboards_use_the_available_height() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);

        for (width, height) in [(80, 24), (100, 32)] {
            app.screen = Screen::Home;
            let home = rendered_text(&app, width, height);
            assert!(
                last_used_row(&home, width as usize) >= height as usize - 2,
                "home leaves the bottom of {width}x{height} unused"
            );

            app.screen = Screen::Measure;
            let measure = rendered_text(&app, width, height);
            for title in [
                "PERFORMANCE / SPECULATION",
                "INFERENCE SIGNALS",
                "INFERENCE PATH",
                "REQUEST HISTORY",
            ] {
                assert!(measure.contains(title), "{width}x{height} omits {title}");
            }

            app.screen = Screen::System;
            let system = rendered_text(&app, width, height);
            for title in [
                "MEMORY STACK",
                "SYSTEM PRESSURE",
                "BLOAT CHECK",
                "ENDPOINTS / PROVENANCE",
            ] {
                assert!(system.contains(title), "{width}x{height} omits {title}");
            }
        }
    }

    #[test]
    fn panel_focus_is_visible_and_enter_opens_one_panel() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        app.show_screen(Screen::Measure);

        let grid = rendered_text(&app, 80, 24);
        assert!(grid.contains("Panel 1/4"));
        assert!(grid.contains("1/4 | Enter open"));
        assert!(grid.contains("PERFORMANCE / SPECULATION"));
        assert!(grid.contains("INFERENCE SIGNALS"));

        app.expand_selected_panel();
        let expanded = rendered_text(&app, 100, 24);
        assert!(expanded.contains("1/2 | Tab details"));
        assert!(expanded.contains("FULL EVIDENCE"));
        assert!(expanded.contains("Shift-Tab previous"));
        assert!(expanded.contains("Esc back"));
        assert!(expanded.contains("DETAIL / ACTIONS"));
        assert!(expanded.contains("PERFORMANCE / SPECULATION"));
        assert!(!expanded.contains("INFERENCE SIGNALS"));

        app.expanded_pane = ExpandedPane::Guide;
        let guide_focused = rendered_text(&app, 100, 24);
        assert!(guide_focused.contains("2/2 | j/k choose | Enter run"));
    }

    #[test]
    fn every_expanded_panel_uses_the_viewport_for_specific_evidence() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        let cases = [
            (Screen::Home, 0, "RUNTIME IDENTITY"),
            (Screen::Home, 1, "DEVICE CAPACITY"),
            (Screen::Home, 2, "RESPONDING ENDPOINTS"),
            (Screen::Home, 3, "CURRENT CUE"),
            (Screen::Measure, 0, "MEASUREMENT CUSTODY"),
            (Screen::Measure, 1, "TRACKED INFERENCE SIGNALS"),
            (Screen::Measure, 2, "LATEST REQUEST"),
            (Screen::Measure, 3, "LOCAL REQUEST LEDGER"),
            (Screen::System, 0, "HOST MEMORY ACCOUNTING"),
            (Screen::System, 1, "SYSTEM CONDITIONS"),
            (Screen::System, 2, "BOUNDED LOCAL SCAN"),
            (Screen::System, 3, "RESPONDING SERVERS"),
        ];

        for (screen, selected, evidence) in cases {
            app.show_screen(screen);
            app.panel_sel = selected;
            app.expand_selected_panel();
            let text = rendered_text(&app, 120, 32);
            assert!(
                text.contains("FULL EVIDENCE"),
                "{screen:?} panel {selected}"
            );
            assert!(text.contains(evidence), "{screen:?} panel {selected}");
            assert!(
                text.contains("DETAIL / ACTIONS"),
                "{screen:?} panel {selected}"
            );
            assert!(
                last_used_row(&text, 120) >= 30,
                "{screen:?} panel {selected} leaves expanded space unused"
            );
        }
    }

    #[test]
    fn narrow_expanded_views_tab_between_full_size_content_and_actions() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        app.show_screen(Screen::System);
        app.expand_selected_panel();

        let content = rendered_text(&app, 64, 16);
        assert!(content.contains("FULL EVIDENCE"));
        assert!(content.contains("1/2 | Tab details"));
        assert!(!content.contains("DETAIL / ACTIONS"));

        app.expanded_pane = ExpandedPane::Guide;
        let guide = rendered_text(&app, 64, 16);
        assert!(guide.contains("DETAIL / ACTIONS [FOCUSED]"));
        assert!(guide.contains("2/2 | j/k choose | Enter run"));
        assert!(!guide.contains("FULL EVIDENCE"));
    }

    #[test]
    fn panel_position_counts_only_visible_panels() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        app.cfg.layout.hidden_panels.push("streams".into());
        app.show_screen(Screen::Measure);

        assert_eq!(app.selected_panel_position(), Some((1, 3)));
        app.cycle_panel(false);
        assert_eq!(app.selected_panel(), Some(FocusPanel::Stages));
        assert_eq!(app.selected_panel_position(), Some((2, 3)));
    }

    #[test]
    fn narrow_terminals_render_only_the_focused_panel() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        app.show_screen(Screen::System);

        let memory = rendered_text(&app, 64, 16);
        assert!(memory.contains("MEMORY STACK"));
        assert!(!memory.contains("SYSTEM PRESSURE"));

        app.cycle_panel(false);
        let pressure = rendered_text(&app, 64, 16);
        assert!(pressure.contains("SYSTEM PRESSURE"));
        assert!(!pressure.contains("MEMORY STACK"));
    }

    #[test]
    fn normal_terminal_popups_keep_list_and_detail_visible() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);

        app.popup = Popup::Models;
        app.model_tab = ModelTab::HuggingFace;
        let models = rendered_text(&app, 80, 24);
        assert!(models.contains("VERIFIED DOWNLOAD"));

        app.popup = Popup::Connect;
        app.connect_model = "test-model".into();
        let agents = rendered_text(&app, 80, 24);
        assert!(agents.contains("found | filter:"));
        assert!(agents.contains("PREPARED"));
    }

    #[test]
    fn command_palette_renders_without_a_footer_dependency() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        app.popup = Popup::Command;
        let backend = TestBackend::new(64, 16);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| ui::render(frame, &app))
            .expect("command palette should render");
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("COMMANDS"));
        assert!(!text.contains("KEYS:"));
    }

    #[test]
    fn model_hub_keeps_one_focused_source_visible_in_small_terminals() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        app.popup = Popup::Models;

        for tab in [ModelTab::Local, ModelTab::HuggingFace, ModelTab::LocalAi] {
            app.model_tab = tab;
            let backend = TestBackend::new(64, 16);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| ui::render(frame, &app))
                .expect("model source should render");
            let text = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("MODELS"));
            match tab {
                ModelTab::Local => assert!(text.contains("IDLE")),
                ModelTab::HuggingFace => assert!(text.contains("SmolLM2")),
                ModelTab::LocalAi => assert!(text.contains("LOCAL.AI")),
            }
        }
    }

    #[test]
    fn agent_setup_popup_uses_the_full_small_terminal_width() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        app.popup = Popup::Connect;
        app.connect_model = "test-model".into();
        let backend = TestBackend::new(64, 16);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| ui::render(frame, &app))
            .expect("agent setup should render");
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("AGENTS"));
        assert!(text.contains("PREPARED"));
    }

    #[test]
    fn observability_focus_changes_only_the_compact_signal_selection() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        app.show_screen(Screen::Measure);
        app.tok_hist.push_back(31.0);
        app.prefill_hist.push_back(420.0);
        app.ttft_hist.push_back(380.0);
        app.kv_hist.push_back(72.0);
        app.queue_hist.push_back(3.0);
        app.acceptance_hist.push_back(68.0);

        for (focus, expected) in [
            ("balanced", ["decode", "TTFT", "KV use"]),
            ("latency", ["TTFT", "decode", "waiting"]),
            ("throughput", ["decode", "prefill", "waiting"]),
            ("memory", ["KV use", "waiting", "engine"]),
            ("speculation", ["accept", "decode", "KV use"]),
        ] {
            app.cfg.observability.focus = focus.into();
            let text = rendered_text(&app, 100, 32);
            assert!(text.contains(&format!("focus {focus}")));
            for label in expected {
                assert!(text.contains(label), "{focus} omits {label}");
            }
        }

        app.panel_sel = 1;
        app.expand_selected_panel();
        let expanded = rendered_text(&app, 120, 32);
        for signal in [
            "decode",
            "prefill",
            "TTFT",
            "KV use",
            "waiting",
            "acceptance",
            "engine CPU",
        ] {
            assert!(
                expanded.contains(signal),
                "expanded evidence omits {signal}"
            );
        }
    }

    #[test]
    fn expanded_performance_shows_concurrency_pressure_provenance_and_budgets() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        config.benchmark.budgets.push(settings::WorkloadBudget {
            workload: "concurrency sweep".into(),
            min_system_tokens_per_second: Some(50.0),
            ..Default::default()
        });
        let mut app = App::new(config);
        app.bench.label = "concurrency sweep".into();
        app.bench.concurrency_results.push(ConcurrencyRun {
            concurrency: 4,
            completed: 4,
            errors: 0,
            wall_ms: 900.0,
            system_tokens_per_second: 53.3,
            mean_request_tokens_per_second: 18.2,
            p95_latency_ms: 870.0,
            p95_tpot_ms: 54.0,
            peak_waiting_requests: Some(2),
            peak_kv_cache_usage: Some(0.74),
            peak_server_rss_gib: 21.5,
            peak_swap_mib: 0.0,
            min_headroom_gib: 31.0,
            token_count_source: "server-reported usage".into(),
        });
        app.show_screen(Screen::Measure);
        app.expand_selected_panel();
        let text = rendered_text(&app, 120, 40);
        assert!(text.contains("CONCURRENCY SWEEP"));
        assert!(text.contains("WORKLOAD BUDGETS"));
        assert!(text.contains("server-reported usage"));
        assert!(text.contains("system_throughput"));
    }

    #[test]
    fn setup_exposes_bounded_session_tracking_controls() {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        config.observability.history_samples = 999;
        config.observability.request_retention = 1;
        let mut app = App::new(config);
        assert_eq!(app.tok_hist.len(), 240);
        assert_eq!(app.cfg.observability.request_retention(), 8);

        app.show_screen(Screen::Customize);
        app.settings_sel = 6;
        let setup = rendered_text(&app, 100, 32);
        assert!(setup.contains("signals"));
        assert!(setup.contains("history"));
        assert!(setup.contains("requests"));
        assert!(setup.contains("session"));
    }

    #[test]
    fn public_report_removes_local_model_path() {
        let mut app = App::new(Config::default());
        app.online = true;
        app.model = "/home/private/models/qwen-8bit".into();
        let report = report::benchmark_markdown(&app);
        assert!(!report.contains("/home/private"));
        assert!(report.contains("qwen-8bit"));
    }

    #[test]
    fn repository_surface_has_no_identity_or_host_paths() {
        fn visit(path: &Path, files: &mut Vec<PathBuf>) {
            let Ok(entries) = fs::read_dir(path) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                if path.is_dir() {
                    if !matches!(name, "target" | ".git") {
                        visit(&path, files);
                    }
                } else if matches!(
                    path.extension().and_then(|value| value.to_str()),
                    Some("rs" | "md" | "toml" | "json" | "lock")
                ) || name == ".gitignore"
                {
                    files.push(path);
                }
            }
        }

        let markers = [
            ["th", "eo"].concat(),
            ["per", "isic"].concat(),
            ["/", "Users", "/"].concat(),
            ["founder", "-mode"].concat(),
            ["@", "gmail"].concat(),
            ["@", "icloud"].concat(),
        ];
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        visit(root, &mut files);
        for path in files {
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let lower = content.to_lowercase();
            for marker in &markers {
                assert!(
                    !lower.contains(&marker.to_lowercase()),
                    "private marker found in {}",
                    path.strip_prefix(root).unwrap_or(&path).display()
                );
            }
        }
    }
}
