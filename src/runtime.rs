use serde::Deserialize;
use std::{
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread,
    time::{Duration, Instant},
};

const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Clone, Default)]
pub struct EngineMetrics {
    pub runtime_version: Option<String>,
    pub kv_cache_tokens: Option<u64>,
    pub kv_cache_usage: Option<f64>,
    pub kv_cache_resident_tokens: Option<u64>,
    pub kv_cache_evictions: Option<u64>,
    pub requests_running: Option<u64>,
    pub requests_waiting: Option<u64>,
    pub requests_swapped: Option<u64>,
    pub prompt_tokens_total: Option<u64>,
    pub prompt_seconds_total: Option<f64>,
    pub predicted_tokens_total: Option<u64>,
    pub predicted_seconds_total: Option<f64>,
    pub mode: Option<String>,
    pub mean_accept_len: Option<f64>,
    pub mean_tokens_per_sec: Option<f64>,
    pub latest_prefill_tok_s: Option<f64>,
    pub latest_decode_tok_s: Option<f64>,
    pub latest_request_tok_s: Option<f64>,
    pub rounds: Option<u64>,
    pub committed_tokens: Option<u64>,
    pub draft_acceptance: Option<f64>,
    pub lookup_rounds: Option<u64>,
    pub position_acceptance: Vec<f64>,
    pub prefix_cached_tokens: Option<u64>,
    pub prefix_queries: Option<u64>,
    pub prefix_hits: Option<u64>,
    pub prefix_partial_hits: Option<u64>,
    pub prefix_reused_tokens: Option<u64>,
    pub memory_active_bytes: Option<u64>,
    pub memory_peak_bytes: Option<u64>,
    pub memory_cache_bytes: Option<u64>,
    pub batch_max: Option<u64>,
    pub batch_requests: Option<u64>,
    pub batch_batches: Option<u64>,
}

#[derive(Deserialize, Clone, Default)]
pub struct RoundEvent {
    #[serde(default)]
    pub drafted: u64,
    #[serde(default)]
    pub accepted: u64,
    #[serde(default)]
    pub committed: u64,
    #[serde(default)]
    pub cap: Option<u32>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub ms: f64,
}

#[derive(Clone)]
pub struct ServedModel {
    pub port: u16,
    pub runtime: String,
    pub model: String,
    pub state: String,
    pub owner: Option<String>,
    pub mode: Option<String>,
    pub target: Option<String>,
    pub drafter: Option<String>,
}

impl ServedModel {
    pub fn endpoint_label(&self) -> String {
        format!(":{}", self.port)
    }
}

#[derive(Clone)]
pub struct ModelSource {
    pub runtime: String,
    pub endpoint: String,
    pub label: String,
    pub state: String,
    pub detail: String,
}

#[derive(Clone)]
pub struct PrimaryEndpoint {
    pub port: u16,
    pub runtime: String,
    pub model: String,
    pub ping_ms: f64,
}

#[derive(Default)]
pub struct Snapshot {
    pub primary: Option<PrimaryEndpoint>,
    pub served: Vec<ServedModel>,
    pub model_sources: Vec<ModelSource>,
    pub metrics: EngineMetrics,
    pub latest_round: Option<RoundEvent>,
    pub prefill_tokens_per_second: Option<f64>,
    pub decode_tokens_per_second: Option<f64>,
    pub active_model_memory_gib: Option<f64>,
    pub quantization: Option<String>,
    pub parameters: Option<String>,
    pub context_limit: Option<u32>,
}

pub struct Probe {
    request_tx: SyncSender<()>,
    snapshot_rx: Receiver<Snapshot>,
}

impl Probe {
    pub fn new(ports: Vec<u16>) -> Self {
        let (request_tx, request_rx) = mpsc::sync_channel(1);
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut roster = Roster::new(ports);
            while request_rx.recv().is_ok() {
                if snapshot_tx.send(roster.poll()).is_err() {
                    break;
                }
            }
        });
        let probe = Self {
            request_tx,
            snapshot_rx,
        };
        probe.request();
        probe
    }

    pub fn request(&self) {
        match self.request_tx.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) | Err(TrySendError::Disconnected(())) => {}
        }
    }

    pub fn take_latest(&self) -> Option<Snapshot> {
        let mut latest = None;
        loop {
            match self.snapshot_rx.try_recv() {
                Ok(snapshot) => latest = Some(snapshot),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return latest,
            }
        }
    }

    pub fn wait_latest(&self, timeout: Duration) -> Option<Snapshot> {
        let first = self.snapshot_rx.recv_timeout(timeout).ok()?;
        let mut latest = first;
        while let Ok(snapshot) = self.snapshot_rx.try_recv() {
            latest = snapshot;
        }
        Some(latest)
    }
}

struct Roster {
    ports: Vec<u16>,
    previous_metrics: Option<(u16, String, EngineMetrics, Instant)>,
}

impl Roster {
    fn new(ports: Vec<u16>) -> Self {
        Self {
            ports,
            previous_metrics: None,
        }
    }

    fn poll(&mut self) -> Snapshot {
        let mut snapshot = Snapshot::default();
        for &port in &self.ports {
            if port == 11434 {
                poll_ollama(port, &mut snapshot);
            } else {
                poll_openai_compatible(port, &mut snapshot);
            }
        }
        self.poll_primary_telemetry(&mut snapshot);
        snapshot
    }

    fn poll_primary_telemetry(&mut self, snapshot: &mut Snapshot) {
        let Some(primary) = snapshot.primary.as_ref() else {
            self.previous_metrics = None;
            snapshot.metrics = EngineMetrics::default();
            return;
        };
        if primary.port == 11434 {
            self.previous_metrics = None;
            return;
        }
        snapshot.metrics = EngineMetrics::default();
        let base = format!("http://127.0.0.1:{}", primary.port);
        if let Some(health) = http_text(&format!("{base}/health"), 400) {
            snapshot.context_limit = parse_health_context(&health);
        }
        if let Some(text) = http_text(&format!("{base}/metrics"), 400) {
            let metrics = parse_metrics(&text);
            snapshot.prefill_tokens_per_second =
                metrics.latest_prefill_tok_s.filter(|rate| *rate > 0.0);
            snapshot.decode_tokens_per_second = metrics
                .mean_tokens_per_sec
                .or(metrics.latest_decode_tok_s)
                .or(metrics.latest_request_tok_s)
                .filter(|rate| *rate > 0.0);

            if let Some((port, model, previous, at)) = &self.previous_metrics {
                if *port == primary.port && model == &primary.model {
                    let elapsed = at.elapsed().as_secs_f64();
                    if elapsed > 0.5 {
                        snapshot.prefill_tokens_per_second = counter_rate(
                            metrics.prompt_tokens_total,
                            previous.prompt_tokens_total,
                            metrics.prompt_seconds_total,
                            previous.prompt_seconds_total,
                        )
                        .or(snapshot.prefill_tokens_per_second);
                        snapshot.decode_tokens_per_second = counter_rate(
                            metrics.predicted_tokens_total,
                            previous.predicted_tokens_total,
                            metrics.predicted_seconds_total,
                            previous.predicted_seconds_total,
                        )
                        .or(snapshot.decode_tokens_per_second);
                    }
                }
            }
            self.previous_metrics = Some((
                primary.port,
                primary.model.clone(),
                metrics.clone(),
                Instant::now(),
            ));
            snapshot.metrics = metrics;
        }
        if snapshot.metrics.runtime_version.is_none() {
            snapshot.metrics.runtime_version =
                http_json::<RuntimeVersion>(&format!("{base}/version"), 400)
                    .and_then(|reading| reading.version);
        }
        if let Some(rounds) = http_json::<RoundsResponse>(&format!("{base}/rounds?limit=1"), 400) {
            snapshot.latest_round = rounds.rounds.into_iter().last();
        }
        if let Some(props) = http_text(&format!("{base}/props"), 400) {
            snapshot.context_limit = parse_props_context(&props).or(snapshot.context_limit);
        }
    }
}

fn counter_rate(
    current_tokens: Option<u64>,
    previous_tokens: Option<u64>,
    current_seconds: Option<f64>,
    previous_seconds: Option<f64>,
) -> Option<f64> {
    let tokens = current_tokens?.checked_sub(previous_tokens?)? as f64;
    let seconds = current_seconds? - previous_seconds?;
    (tokens > 0.0 && seconds > 0.0).then_some(tokens / seconds)
}

fn poll_ollama(port: u16, snapshot: &mut Snapshot) {
    snapshot.metrics.runtime_version =
        http_json::<RuntimeVersion>(&format!("http://127.0.0.1:{port}/api/version"), 400)
            .and_then(|reading| reading.version);
    let tags = http_json::<OllamaTags>(&format!("http://127.0.0.1:{port}/api/tags"), 400)
        .unwrap_or_default();
    let active = http_json::<OllamaProcesses>(&format!("http://127.0.0.1:{port}/api/ps"), 400)
        .unwrap_or_default();

    for model in &tags.models {
        let detail = format!(
            "installed{}{}{}",
            model
                .size
                .map(|size| format!(" | {:.1} GiB", size as f64 / BYTES_PER_GIB))
                .unwrap_or_default(),
            model
                .details
                .as_ref()
                .and_then(|details| details.parameter_size.as_deref())
                .map(|parameters| format!(" | {parameters}"))
                .unwrap_or_default(),
            model
                .details
                .as_ref()
                .and_then(|details| details.quantization_level.as_deref())
                .map(|quantization| format!(" | {quantization}"))
                .unwrap_or_default()
        );
        snapshot.model_sources.push(ModelSource {
            runtime: "Ollama".into(),
            endpoint: format!(":{port}"),
            label: model.name.clone(),
            state: "installed".into(),
            detail,
        });
    }

    if active.models.is_empty() {
        if !tags.models.is_empty() {
            snapshot.served.push(ServedModel {
                port,
                runtime: "Ollama".into(),
                model: "no model loaded".into(),
                state: "idle".into(),
                owner: Some("Ollama".into()),
                mode: None,
                target: None,
                drafter: None,
            });
        }
        return;
    }

    for model in &active.models {
        let label = model
            .name
            .clone()
            .or_else(|| model.model.clone())
            .unwrap_or_else(|| "unknown model".into());
        snapshot.model_sources.push(ModelSource {
            runtime: "Ollama".into(),
            endpoint: format!(":{port}"),
            label: label.clone(),
            state: "loaded".into(),
            detail: "active now".into(),
        });
        snapshot.served.push(ServedModel {
            port,
            runtime: "Ollama".into(),
            model: label.clone(),
            state: "loaded".into(),
            owner: Some("Ollama".into()),
            mode: None,
            target: None,
            drafter: None,
        });
        if snapshot.primary.is_none() {
            snapshot.primary = Some(PrimaryEndpoint {
                port,
                runtime: "Ollama".into(),
                model: label,
                ping_ms: 0.0,
            });
            snapshot.active_model_memory_gib =
                model.size_vram.map(|bytes| bytes as f64 / BYTES_PER_GIB);
            snapshot.context_limit = model.context_length.map(|value| value as u32);
            if let Some(details) = &model.details {
                snapshot.quantization = details.quantization_level.clone();
                snapshot.parameters = details.parameter_size.clone();
            }
        }
    }
}

fn poll_openai_compatible(port: u16, snapshot: &mut Snapshot) {
    let started = Instant::now();
    let Some(response) =
        http_json::<ModelsResponse>(&format!("http://127.0.0.1:{port}/v1/models"), 400)
    else {
        return;
    };
    let ping_ms = started.elapsed().as_secs_f64() * 1000.0;
    if response.data.is_empty() {
        let status =
            http_json::<AdminStatus>(&format!("http://127.0.0.1:{port}/admin/status"), 400)
                .unwrap_or_default();
        let state = if status.loading == Some(true) {
            "loading"
        } else if status.ready == Some(true) {
            "loaded"
        } else if status.error.is_some() {
            "error"
        } else {
            "no model"
        };
        let model = status.model.unwrap_or_else(|| "no model".into());
        let runtime = endpoint_runtime(port).to_string();
        snapshot.served.push(ServedModel {
            port,
            runtime: runtime.clone(),
            model: model.clone(),
            state: state.into(),
            owner: Some("local server".into()),
            mode: None,
            target: None,
            drafter: None,
        });
        if snapshot.primary.is_none() && state != "no model" {
            snapshot.primary = Some(PrimaryEndpoint {
                port,
                runtime,
                model,
                ping_ms,
            });
        }
        return;
    }

    for model in &response.data {
        let spec = model.dspark.clone().unwrap_or_default();
        let runtime = if model.dspark.is_some() {
            "mlx-dspark".to_string()
        } else {
            endpoint_runtime(port).to_string()
        };
        let label = model
            .display_name
            .clone()
            .unwrap_or_else(|| model.id.clone());
        snapshot.model_sources.push(ModelSource {
            runtime: runtime.clone(),
            endpoint: format!(":{port}"),
            label: label.clone(),
            state: "loaded".into(),
            detail: "responding endpoint".into(),
        });
        snapshot.served.push(ServedModel {
            port,
            runtime: runtime.clone(),
            model: label,
            state: "loaded".into(),
            owner: model.owned_by.clone().or_else(|| Some("OpenAI API".into())),
            mode: spec.mode,
            target: spec.target,
            drafter: spec.drafter,
        });
        if snapshot.primary.is_none() {
            snapshot.primary = Some(PrimaryEndpoint {
                port,
                runtime,
                model: model
                    .id
                    .split('/')
                    .next_back()
                    .unwrap_or("ready")
                    .to_string(),
                ping_ms,
            });
        }
    }
}

pub fn endpoint_runtime(port: u16) -> &'static str {
    match port {
        11434 => "Ollama",
        1234 => "LM Studio",
        8080 | 8000 => "llama.cpp / OpenAI-compatible",
        _ => "OpenAI-compatible",
    }
}

pub fn post_model_load(port: u16, target: &str) -> Result<(), String> {
    let body = serde_json::json!({"model": target, "mode": "auto"}).to_string();
    let response = ureq::post(&format!("http://127.0.0.1:{port}/admin/load"))
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(5))
        .send_string(&body)
        .map_err(|error| error.to_string())?;
    if response.status() >= 400 {
        return Err(format!("server returned HTTP {}", response.status()));
    }
    Ok(())
}

fn http_json<T: for<'de> Deserialize<'de>>(url: &str, timeout_ms: u64) -> Option<T> {
    ureq::get(url)
        .timeout(Duration::from_millis(timeout_ms))
        .call()
        .ok()?
        .into_json::<T>()
        .ok()
}

fn http_text(url: &str, timeout_ms: u64) -> Option<String> {
    ureq::get(url)
        .timeout(Duration::from_millis(timeout_ms))
        .call()
        .ok()?
        .into_string()
        .ok()
}

pub fn parse_metrics(text: &str) -> EngineMetrics {
    let mut metrics = EngineMetrics::default();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        if value.is_object() {
            metrics.runtime_version = value["version"].as_str().map(str::to_string);
            metrics.mode = value["mode"].as_str().map(str::to_string);
            metrics.mean_accept_len = value["mean_accept_len"].as_f64();
            metrics.mean_tokens_per_sec = value["mean_tokens_per_sec"].as_f64();
            if let Some(latest) = value.get("latest") {
                metrics.latest_prefill_tok_s = latest["prefill_tok_s"].as_f64();
                metrics.latest_decode_tok_s = latest["decode_tok_s"].as_f64();
                metrics.latest_request_tok_s = latest["request_tok_s"].as_f64();
            }
            metrics.kv_cache_tokens = value["kv_cache_tokens"].as_u64();
            metrics.kv_cache_resident_tokens = value["kv_cache_resident_tokens"]
                .as_u64()
                .or_else(|| value["kv_cache"]["resident_tokens"].as_u64());
            metrics.kv_cache_evictions = value["kv_cache_evictions"]
                .as_u64()
                .or_else(|| value["kv_cache"]["evictions"].as_u64());
            metrics.kv_cache_usage = value["kv_cache_usage"]
                .as_f64()
                .or_else(|| value["kv_cache"]["usage"].as_f64())
                .map(|usage| if usage > 1.0 { usage / 100.0 } else { usage });
            let scheduler = &value["scheduler"];
            metrics.requests_running = scheduler["running"].as_u64();
            metrics.requests_waiting = scheduler["waiting"].as_u64();
            metrics.requests_swapped = scheduler["swapped"].as_u64();

            let rounds = &value["rounds"];
            metrics.rounds = rounds["rounds"].as_u64();
            metrics.committed_tokens = rounds["committed_tokens"].as_u64();
            metrics.draft_acceptance = rounds["draft_acceptance"].as_f64();
            metrics.lookup_rounds = rounds["lookup_rounds"].as_u64();
            metrics.position_acceptance = rounds["position_acceptance"]
                .as_array()
                .map(|values| values.iter().filter_map(|value| value.as_f64()).collect())
                .unwrap_or_default();

            let prefix = &value["prefix_cache"];
            metrics.prefix_cached_tokens = prefix["cached_tokens"].as_u64();
            metrics.prefix_queries = prefix["queries"].as_u64();
            metrics.prefix_hits = prefix["hits"].as_u64();
            metrics.prefix_partial_hits = prefix["partial_hits"].as_u64();
            metrics.prefix_reused_tokens = prefix["reused_tokens"].as_u64();

            let memory = &value["memory"];
            metrics.memory_active_bytes = memory["active_bytes"].as_u64();
            metrics.memory_peak_bytes = memory["peak_bytes"].as_u64();
            metrics.memory_cache_bytes = memory["cache_bytes"].as_u64();

            let batching = &value["batching"];
            metrics.batch_max = batching["max_batch"].as_u64();
            metrics.batch_requests = batching["batched_requests"].as_u64();
            metrics.batch_batches = batching["batches"].as_u64();
            return metrics;
        }
    }

    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(raw_key) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else { continue };
        let key = raw_key.split('{').next().unwrap_or(raw_key);
        match key {
            "vllm:build_info" | "llamacpp:build_info" => {
                metrics.runtime_version = prometheus_label_value(raw_key, "version")
            }
            "llamacpp:kv_cache_tokens" => metrics.kv_cache_tokens = value.parse().ok(),
            "llamacpp:kv_cache_resident_tokens" | "vllm:kv_cache_resident_tokens" => {
                metrics.kv_cache_resident_tokens = value.parse().ok()
            }
            "llamacpp:kv_cache_evictions_total" | "vllm:kv_cache_evictions_total" => {
                metrics.kv_cache_evictions = value.parse().ok()
            }
            "llamacpp:prompt_tokens_total" => metrics.prompt_tokens_total = value.parse().ok(),
            "llamacpp:prompt_seconds_total" => metrics.prompt_seconds_total = value.parse().ok(),
            "llamacpp:tokens_predicted_total" => {
                metrics.predicted_tokens_total = value.parse().ok()
            }
            "llamacpp:predicted_seconds_total" => {
                metrics.predicted_seconds_total = value.parse().ok()
            }
            "vllm:kv_cache_usage_perc" | "vllm:gpu_cache_usage_perc" => {
                metrics.kv_cache_usage =
                    value.parse::<f64>().ok().map(
                        |usage| {
                            if usage > 1.0 {
                                usage / 100.0
                            } else {
                                usage
                            }
                        },
                    )
            }
            "vllm:num_requests_running" => metrics.requests_running = value.parse().ok(),
            "vllm:num_requests_waiting" => metrics.requests_waiting = value.parse().ok(),
            "vllm:num_requests_swapped" => metrics.requests_swapped = value.parse().ok(),
            "vllm:prefix_cache_queries" => metrics.prefix_queries = value.parse().ok(),
            "vllm:prefix_cache_hits" => metrics.prefix_hits = value.parse().ok(),
            "vllm:prompt_tokens_total" => metrics.prompt_tokens_total = value.parse().ok(),
            "vllm:generation_tokens_total" => metrics.predicted_tokens_total = value.parse().ok(),
            _ => {}
        }
    }
    metrics
}

fn prometheus_label_value(raw_key: &str, wanted: &str) -> Option<String> {
    let labels = raw_key.split_once('{')?.1.strip_suffix('}')?;
    labels.split(',').find_map(|label| {
        let (key, value) = label.split_once('=')?;
        (key.trim() == wanted).then(|| value.trim().trim_matches('"').to_string())
    })
}

pub fn parse_health_context(text: &str) -> Option<u32> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value["effective_context_limit"]
        .as_u64()
        .or(value["context_window"].as_u64())
        .or(value["loaded_context_size"].as_u64())
        .map(|number| number.min(u32::MAX as u64) as u32)
}

pub fn parse_props_context(text: &str) -> Option<u32> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value["default_generation_settings"]["n_ctx"]
        .as_u64()
        .or(value["n_ctx"].as_u64())
        .map(|number| number as u32)
}

#[derive(Deserialize, Default)]
struct RuntimeVersion {
    version: Option<String>,
}

#[derive(Deserialize, Default)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelId>,
}

#[derive(Deserialize, Clone, Default)]
struct DsparkModelInfo {
    mode: Option<String>,
    target: Option<String>,
    drafter: Option<String>,
}

#[derive(Deserialize, Clone)]
struct ModelId {
    id: String,
    #[serde(default)]
    owned_by: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(rename = "x_mlx_dspark", default)]
    dspark: Option<DsparkModelInfo>,
}

#[derive(Deserialize, Default)]
struct AdminStatus {
    ready: Option<bool>,
    loading: Option<bool>,
    model: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize, Default)]
struct OllamaProcesses {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[derive(Deserialize, Clone)]
struct OllamaModel {
    name: Option<String>,
    model: Option<String>,
    size_vram: Option<u64>,
    context_length: Option<u64>,
    details: Option<OllamaDetails>,
}

#[derive(Deserialize, Default)]
struct OllamaTags {
    #[serde(default)]
    models: Vec<OllamaModelTag>,
}

#[derive(Deserialize)]
struct OllamaModelTag {
    name: String,
    size: Option<u64>,
    details: Option<OllamaDetails>,
}

#[derive(Deserialize, Clone)]
struct OllamaDetails {
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

#[derive(Deserialize, Default)]
struct RoundsResponse {
    #[serde(default)]
    rounds: Vec<RoundEvent>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mlx_dspark_metrics() {
        let metrics = parse_metrics(
            r#"{"model":"local","mode":"dflash","requests":9,"mean_accept_len":5.92,"mean_tokens_per_sec":34.8,"memory":{"active_bytes":123}}"#,
        );
        assert_eq!(metrics.mode.as_deref(), Some("dflash"));
        assert_eq!(metrics.mean_accept_len, Some(5.92));
        assert_eq!(metrics.mean_tokens_per_sec, Some(34.8));
    }

    #[test]
    fn parses_mlx_vlm_latest_rates() {
        let metrics = parse_metrics(
            r#"{"latest":{"prefill_tok_s":49.5,"decode_tok_s":12.4,"request_tok_s":11.8}}"#,
        );
        assert_eq!(metrics.latest_prefill_tok_s, Some(49.5));
        assert_eq!(metrics.latest_decode_tok_s, Some(12.4));
        assert_eq!(metrics.latest_request_tok_s, Some(11.8));
    }

    #[test]
    fn parses_speculation_allocator_and_prefix_details() {
        let metrics = parse_metrics(
            r#"{
                "mode":"dflash",
                "rounds":{"rounds":205,"committed_tokens":1164,"draft_acceptance":0.6683,"lookup_rounds":0,"position_acceptance":[0.97,0.88]},
                "prefix_cache":{"cached_tokens":39,"hits":6,"partial_hits":1,"reused_tokens":162},
                "memory":{"active_bytes":30000000000,"peak_bytes":33000000000,"cache_bytes":500000000},
                "batching":{"max_batch":2,"batched_requests":4,"batches":2}
            }"#,
        );
        assert_eq!(metrics.rounds, Some(205));
        assert_eq!(metrics.draft_acceptance, Some(0.6683));
        assert_eq!(metrics.position_acceptance, vec![0.97, 0.88]);
        assert_eq!(metrics.prefix_hits, Some(6));
        assert_eq!(metrics.prefix_reused_tokens, Some(162));
        assert_eq!(metrics.memory_peak_bytes, Some(33000000000));
        assert_eq!(metrics.batch_max, Some(2));
    }

    #[test]
    fn parses_vllm_scheduler_cache_and_labeled_metrics() {
        let metrics = parse_metrics(
            r#"
                vllm:build_info{version="0.11.2",model_name="local"} 1
                vllm:kv_cache_usage_perc{model_name="local"} 0.73
                vllm:kv_cache_resident_tokens{model_name="local"} 16384
                vllm:kv_cache_evictions_total{model_name="local"} 3
                vllm:num_requests_running{model_name="local"} 2
                vllm:num_requests_waiting{model_name="local"} 4
                vllm:num_requests_swapped{model_name="local"} 1
                vllm:prefix_cache_queries{model_name="local"} 20
                vllm:prefix_cache_hits{model_name="local"} 15
            "#,
        );
        assert_eq!(metrics.runtime_version.as_deref(), Some("0.11.2"));
        assert_eq!(metrics.kv_cache_usage, Some(0.73));
        assert_eq!(metrics.kv_cache_resident_tokens, Some(16384));
        assert_eq!(metrics.kv_cache_evictions, Some(3));
        assert_eq!(metrics.requests_running, Some(2));
        assert_eq!(metrics.requests_waiting, Some(4));
        assert_eq!(metrics.requests_swapped, Some(1));
        assert_eq!(metrics.prefix_queries, Some(20));
        assert_eq!(metrics.prefix_hits, Some(15));
    }

    #[test]
    fn parses_context_limits() {
        assert_eq!(
            parse_health_context(r#"{"effective_context_limit":262144}"#),
            Some(262144)
        );
        assert_eq!(
            parse_props_context(r#"{"default_generation_settings":{"n_ctx":8192}}"#),
            Some(8192)
        );
    }

    #[test]
    fn probe_requests_are_single_flight_and_nonblocking() {
        let probe = Probe::new(vec![9]);
        let started = Instant::now();
        for _ in 0..100 {
            probe.request();
        }
        assert!(started.elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn names_common_local_runtimes_explicitly() {
        assert_eq!(endpoint_runtime(11434), "Ollama");
        assert_eq!(endpoint_runtime(1234), "LM Studio");
        assert!(endpoint_runtime(8080).contains("llama.cpp"));
    }
}
