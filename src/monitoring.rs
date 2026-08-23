use super::{eval, public_model_id, App, Stage};

pub(crate) const MONITORING_SCHEMA: &str = "tokoro.monitoring.v1";
pub(crate) const BASELINE_PROFILE: &str = "local_inference_core.v1";

#[derive(Clone, Debug)]
pub(crate) struct Cue {
    pub(crate) code: &'static str,
    pub(crate) severity: &'static str,
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) evidence: String,
    pub(crate) action_label: &'static str,
    pub(crate) action_key: &'static str,
    pub(crate) action_command: Option<&'static str>,
}

#[derive(Clone, Debug)]
struct Layer {
    id: &'static str,
    label: &'static str,
    posture: &'static str,
    current: String,
    standard: &'static [&'static str],
    capabilities: &'static [&'static str],
    gaps: &'static [&'static str],
}

pub(crate) fn primary_cue(app: &App) -> Cue {
    cues(app).into_iter().next().unwrap_or_else(|| Cue {
        code: "ready",
        severity: "ok",
        title: "Ready for local requests".into(),
        detail: "The model endpoint is responding and no reported pressure needs attention.".into(),
        evidence: "local endpoint and host probes".into(),
        action_label: "Connect a local agent",
        action_key: "c",
        action_command: Some("tokoro agents --json"),
    })
}

pub(crate) fn cue_values(app: &App) -> Vec<serde_json::Value> {
    cues(app).iter().map(cue_value).collect()
}

fn cues(app: &App) -> Vec<Cue> {
    let mut cues = Vec::new();

    if let Some(server) = app.served.iter().find(|server| server.state == "error") {
        cues.push(Cue {
            code: "model_load_error",
            severity: "error",
            title: format!("{} could not load the model", server.runtime),
            detail: "The runtime reported an error state. Inspect its status before retrying."
                .into(),
            evidence: format!("runtime-reported state on {}", server.endpoint_label()),
            action_label: "Inspect runtime evidence",
            action_key: "3",
            action_command: Some("tokoro inspect --json"),
        });
        return cues;
    }

    if let Some(server) = app.served.iter().find(|server| server.state == "loading") {
        cues.push(Cue {
            code: "model_loading",
            severity: "info",
            title: format!("Loading {}", public_model_id(&server.model)),
            detail: "The runtime is responding, but the model is not ready for requests yet."
                .into(),
            evidence: format!("runtime-reported state on {}", server.endpoint_label()),
            action_label: "Watch model readiness",
            action_key: "1",
            action_command: Some("tokoro inspect --json"),
        });
        return cues;
    }

    if !app.online {
        cues.push(Cue {
            code: "no_model_running",
            severity: "info",
            title: "No model is running".into(),
            detail: if app.server.available.is_empty() {
                "Inspect local runtimes or download a checked model to begin.".into()
            } else {
                format!(
                    "{} local target{} available. Choose one to start.",
                    app.server.available.len(),
                    if app.server.available.len() == 1 {
                        " is"
                    } else {
                        "s are"
                    }
                )
            },
            evidence: "no loaded model on the configured localhost endpoints".into(),
            action_label: "Choose a local model",
            action_key: "m",
            action_command: Some("tokoro models --refresh --json"),
        });
        return cues;
    }

    if app
        .spans
        .back()
        .is_some_and(|request| request.stage == Stage::Failed)
    {
        cues.push(Cue {
            code: "request_failed",
            severity: "error",
            title: "The latest observed request failed".into(),
            detail: "Open the inference path to inspect its stage and the runtime evidence.".into(),
            evidence: "metrics-only local request ledger".into(),
            action_label: "Inspect the inference path",
            action_key: "2",
            action_command: Some("tokoro inspect --json"),
        });
    }

    if let Some(waiting) = app.metrics.requests_waiting.filter(|waiting| *waiting > 0) {
        cues.push(Cue {
            code: "requests_waiting",
            severity: "warn",
            title: format!(
                "{waiting} request{} waiting",
                if waiting == 1 { " is" } else { "s are" }
            ),
            detail: "First-token latency is exposed to queue pressure until the backlog clears."
                .into(),
            evidence: "runtime-reported scheduler queue".into(),
            action_label: "Inspect queue and throughput",
            action_key: "2",
            action_command: Some("tokoro inspect --json"),
        });
    }

    if app.swap_mb > 500.0 {
        cues.push(Cue {
            code: "active_swap",
            severity: "warn",
            title: format!("Swap pressure is {:.1} GiB", app.swap_mb / 1024.0),
            detail: "Decode may stutter while model pages compete with other processes.".into(),
            evidence: "host-reported used swap".into(),
            action_label: "Inspect memory pressure",
            action_key: "3",
            action_command: Some("tokoro inspect --json"),
        });
    } else if app.headroom_gb < 10.0 && app.total_mem_gb > 0.0 {
        cues.push(Cue {
            code: "low_memory_headroom",
            severity: "warn",
            title: format!("Only {:.1} GiB of RAM is available", app.headroom_gb),
            detail: "A longer context or another loaded model may push the host into swap.".into(),
            evidence: "host memory sample".into(),
            action_label: "Inspect memory pressure",
            action_key: "3",
            action_command: Some("tokoro inspect --json"),
        });
    }

    if let Some(usage) = app.metrics.kv_cache_usage.filter(|usage| *usage >= 0.9) {
        cues.push(Cue {
            code: "kv_near_capacity",
            severity: "warn",
            title: format!("KV cache is {:.0}% occupied", usage * 100.0),
            detail: "New or growing requests may trigger eviction, preemption, or queueing.".into(),
            evidence: "runtime-reported KV capacity".into(),
            action_label: "Inspect cache and scheduler",
            action_key: "2",
            action_command: Some("tokoro inspect --json"),
        });
    }

    if app.interference.low_power {
        cues.push(Cue {
            code: "low_power_mode",
            severity: "warn",
            title: "Low power mode is enabled".into(),
            detail: "Available compute clocks may be lower than a comparable full-power run."
                .into(),
            evidence: "platform power-mode reading".into(),
            action_label: "Inspect system conditions",
            action_key: "3",
            action_command: Some("tokoro inspect --json"),
        });
    }

    if app.bench.active {
        cues.push(Cue {
            code: "benchmark_running",
            severity: "info",
            title: format!("Measuring {}", app.bench.label),
            detail: "Tokoro is sampling latency, throughput, queue, cache, and host pressure."
                .into(),
            evidence: "active local benchmark".into(),
            action_label: "Watch benchmark evidence",
            action_key: "2",
            action_command: None,
        });
    } else if app.bench.results.is_empty() && app.bench.concurrency_results.is_empty() {
        cues.push(Cue {
            code: "baseline_missing",
            severity: "info",
            title: "The model is ready, but no baseline exists".into(),
            detail: "Run the same bounded workload before changing the model or serving setup."
                .into(),
            evidence: "no benchmark measurements in this session".into(),
            action_label: "Run a quick local baseline",
            action_key: "b",
            action_command: Some("tokoro benchmark run \"Quick response\" --json --save"),
        });
    }

    if cues.is_empty() {
        cues.push(Cue {
            code: "ready",
            severity: "ok",
            title: "Ready for local requests".into(),
            detail: "The model endpoint is responding and no reported pressure needs attention."
                .into(),
            evidence: "local endpoint and host probes".into(),
            action_label: "Connect a local agent",
            action_key: "c",
            action_command: Some("tokoro agents --json"),
        });
    }
    cues
}

fn cue_value(cue: &Cue) -> serde_json::Value {
    serde_json::json!({
        "code": cue.code,
        "severity": cue.severity,
        "title": cue.title,
        "detail": cue.detail,
        "evidence": cue.evidence,
        "action": {
            "label": cue.action_label,
            "interactive_key": cue.action_key,
            "command": cue.action_command,
        },
    })
}

fn layers(app: &App) -> Vec<Layer> {
    let request_records = app.spans.len();
    let benchmark_runs = app.bench.results.len() + app.bench.concurrency_results.len();
    let scheduler_available = app.metrics.requests_running.is_some()
        || app.metrics.requests_waiting.is_some()
        || app.metrics.requests_swapped.is_some();
    let cache_available = app.metrics.kv_cache_usage.is_some()
        || app.metrics.kv_cache_resident_tokens.is_some()
        || app.metrics.prefix_queries.is_some();
    let evals = eval::list().unwrap_or_default();
    let reviewed = evals
        .iter()
        .filter(|fixture| matches!(fixture.status.as_str(), "pass" | "fail"))
        .count();

    vec![
        Layer {
            id: "lifecycle",
            label: "Model lifecycle and availability",
            posture: "partial",
            current: if app.online {
                format!(
                    "observed now: {} serving {} on :{}",
                    app.engine,
                    public_model_id(&app.model),
                    app.port
                )
            } else {
                "idle baseline: no loaded model is responding".into()
            },
            standard: &[
                "readiness and health",
                "loaded model and runtime identity",
                "load duration, failures, restarts, and uptime",
            ],
            capabilities: &[
                "live, loading, idle, no-model, and error states",
                "model, runtime, endpoint age, ping, and runtime version",
                "managed start and stop where the runtime supports it",
            ],
            gaps: &[
                "time-to-ready history",
                "restart and uptime counters",
                "structured runtime error reasons",
            ],
        },
        Layer {
            id: "request_experience",
            label: "Request experience",
            posture: "partial",
            current: format!(
                "{request_records} metrics-only request records; {benchmark_runs} measured benchmark points"
            ),
            standard: &[
                "request rate, completion, abort, and error reason",
                "TTFT, TPOT or ITL, and end-to-end latency histograms",
                "queue, prefill, and decode duration decomposition",
            ],
            capabilities: &[
                "request stage, TTFT, TPOT, end-to-end duration, and output count",
                "p50 and p95 benchmark distributions",
                "bounded metrics-only request history",
            ],
            gaps: &[
                "passive request-rate and error-rate counters",
                "runtime-wide latency histograms",
                "queue, prefill, and decode duration decomposition",
            ],
        },
        Layer {
            id: "tokens_throughput",
            label: "Tokens and throughput",
            posture: "strong",
            current: if app.real_tg.is_some() || !app.bench.results.is_empty() {
                "measured now or present in the current benchmark".into()
            } else {
                "supported; awaiting a live measured request".into()
            },
            standard: &[
                "input and output token counts",
                "per-request decode throughput",
                "aggregate system throughput under concurrency",
            ],
            capabilities: &[
                "prefill and decode rates",
                "per-request and aggregate system throughput kept separate",
                "server usage preferred with stream-frame fallback labeled as estimated",
            ],
            gaps: &["passive per-model request-rate series outside measured workloads"],
        },
        Layer {
            id: "scheduler_cache",
            label: "Scheduler and cache",
            posture: "conditional",
            current: format!(
                "scheduler {}; cache {}",
                if scheduler_available {
                    "reported now"
                } else {
                    "not reported by the active runtime"
                },
                if cache_available {
                    "reported now"
                } else {
                    "not reported by the active runtime"
                }
            ),
            standard: &[
                "running, waiting, swapped, and queue duration",
                "KV occupancy, residency, eviction, and preemption",
                "prefix reuse, batching, and speculative acceptance",
            ],
            capabilities: &[
                "vLLM and compatible scheduler gauges",
                "KV usage, residency, evictions, and context capacity kept distinct",
                "prefix, batch, and speculative telemetry where reported",
            ],
            gaps: &[
                "queue-duration histograms",
                "preemption counters across every runtime",
                "Ollama server metrics until Ollama exposes them",
            ],
        },
        Layer {
            id: "host_device",
            label: "Host and accelerator resources",
            posture: "partial",
            current: format!(
                "host sampled now: {:.0}% CPU, {:.1} GiB available, {:.0} MiB swap",
                app.host_cpu_pct, app.headroom_gb, app.swap_mb
            ),
            standard: &[
                "host CPU, RAM, process RSS, swap, and storage",
                "accelerator utilization and memory by device",
                "temperature, power, throttling, and hardware errors",
            ],
            capabilities: &[
                "host RAM, server RSS, headroom, swap, CPU, and model storage",
                "Apple unified-memory semantics kept separate from discrete device memory",
                "macOS low-power and CPU throttle cues",
            ],
            gaps: &[
                "NVIDIA DCGM or equivalent device utilization",
                "device temperature, power, ECC, and XID errors",
                "portable per-device weights, KV, and compute allocation",
            ],
        },
        Layer {
            id: "quality",
            label: "Quality regression",
            posture: "partial",
            current: format!("{} private fixtures; {reviewed} human-reviewed", evals.len()),
            standard: &[
                "versioned golden fixtures",
                "human ground truth and review queues",
                "repeatable regression execution and trend gates",
            ],
            capabilities: &[
                "private content-hashed fixtures",
                "explicit human pass or fail review",
                "measurement provenance separated from private content",
            ],
            gaps: &[
                "one-command fixture execution",
                "quality deltas linked to model and runtime changes",
                "CI regression summaries",
            ],
        },
        Layer {
            id: "agents",
            label: "Agent operations",
            posture: "partial",
            current: format!(
                "{} local clients detected; typed inspection and setup available",
                app.agents.detected().len()
            ),
            standard: &[
                "model and tool-call spans",
                "duration, tokens, status, and error attributes",
                "content capture disabled unless explicitly enabled",
            ],
            capabilities: &[
                "versioned tokoro.agent.v1 JSON commands",
                "local endpoint configuration for detected clients",
                "metrics-only inspection without TUI scraping",
            ],
            gaps: &[
                "metrics-only JSONL event stream",
                "prepared start and stop actions with approval receipts",
                "agent and tool span correlation",
            ],
        },
        Layer {
            id: "interoperability",
            label: "Telemetry interoperability",
            posture: "intentional_boundary",
            current: "verified report packs can render Prometheus text and OTLP JSON".into(),
            standard: &[
                "stable names, units, labels, and provenance",
                "aggregatable histograms for latency",
                "vendor-neutral metrics, traces, and logs",
            ],
            capabilities: &[
                "checked Markdown, JSON, CSV, Prometheus, and OTLP JSON handoffs",
                "no collector or observability account required",
                "missing evidence remains unavailable rather than becoming zero",
            ],
            gaps: &[
                "live scrape endpoint",
                "continuous OTLP delivery",
                "logs and distributed traces",
            ],
        },
        Layer {
            id: "privacy_custody",
            label: "Privacy and custody",
            posture: "strong",
            current: "session metrics only; prompts and responses are not retained".into(),
            standard: &[
                "metrics without prompt content by default",
                "explicit opt-in for sensitive content",
                "bounded retention and clear export custody",
            ],
            capabilities: &[
                "no prompt or response bodies in session telemetry or reports",
                "bounded local request and signal history",
                "checked explicit handoffs with privacy receipts",
            ],
            gaps: &["content-bearing tracing remains intentionally outside the default path"],
        },
    ]
}

pub(crate) fn posture_value(app: &App, agent_schema: &str) -> serde_json::Value {
    let layers = layers(app);
    let count = |posture: &str| {
        layers
            .iter()
            .filter(|layer| layer.posture == posture)
            .count()
    };
    serde_json::json!({
        "schema": agent_schema,
        "kind": "monitoring_posture",
        "monitoring_schema": MONITORING_SCHEMA,
        "profile": BASELINE_PROFILE,
        "checked_date": "2026-08-22",
        "scope": "local model inference from host and runtime through clients and quality review",
        "interpretation": {
            "strong": "first-class support with explicit provenance",
            "conditional": "first-class only when the selected runtime reports the signal",
            "partial": "useful support exists, but an industry-standard part is missing",
            "intentional_boundary": "Tokoro prepares a verified handoff instead of running infrastructure",
            "performance_thresholds": "user_defined_only",
        },
        "summary": {
            "strong": count("strong"),
            "conditional": count("conditional"),
            "partial": count("partial"),
            "intentional_boundary": count("intentional_boundary"),
            "score": serde_json::Value::Null,
        },
        "cues": cue_values(app),
        "layers": layers.iter().map(|layer| serde_json::json!({
            "id": layer.id,
            "label": layer.label,
            "posture": layer.posture,
            "current": layer.current,
            "industry_standard": layer.standard,
            "tokoro_capabilities": layer.capabilities,
            "gaps": layer.gaps,
        })).collect::<Vec<_>>(),
        "basis": [
            {"name": "vLLM metrics", "url": "https://docs.vllm.ai/en/stable/design/metrics/"},
            {"name": "SGLang production metrics", "url": "https://docs.sglang.ai/references/production_metrics.html"},
            {"name": "Hugging Face TGI metrics", "url": "https://huggingface.co/docs/text-generation-inference/reference/metrics"},
            {"name": "OpenTelemetry GenAI observability", "url": "https://opentelemetry.io/blog/2026/genai-observability/"},
            {"name": "NVIDIA GPU telemetry", "url": "https://docs.nvidia.com/datacenter/cloud-native/gpu-telemetry/latest/dcgm-exporter.html"},
            {"name": "Prometheus instrumentation practices", "url": "https://prometheus.io/docs/practices/the_zen/"}
        ],
        "research_method": "official public documentation checked with Firecrawl; local capability audit checked against this build",
        "privacy": "metrics_only_no_prompt_or_response_bodies",
    })
}

pub(crate) fn posture_text(app: &App) -> String {
    let layers = layers(app);
    let mut output = format!(
        "MONITORING POSTURE  {}  checked 2026-08-22\n",
        BASELINE_PROFILE
    );
    output.push_str("No universal performance score; workload thresholds remain user-defined.\n\n");
    for layer in layers {
        let mark = match layer.posture {
            "strong" => "+",
            "conditional" => "?",
            "intentional_boundary" => ">",
            _ => "~",
        };
        output.push_str(&format!(
            "[{mark}] {:<22} {:<20} {}\n",
            layer.id, layer.posture, layer.current
        ));
        if !layer.gaps.is_empty() {
            output.push_str(&format!("    next: {}\n", layer.gaps.join("; ")));
        }
    }
    let cue = primary_cue(app);
    output.push_str(&format!(
        "\nCURRENT CUE  {}  {}\n{}\n",
        cue.severity.to_ascii_uppercase(),
        cue.title,
        cue.detail
    ));
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Config;

    #[test]
    fn monitoring_profile_names_every_stack_layer_without_a_fake_score() {
        let app = App::new(Config::default());
        let payload = posture_value(&app, "tokoro.agent.v1");
        let layer_ids = payload["layers"]
            .as_array()
            .expect("layers")
            .iter()
            .filter_map(|layer| layer["id"].as_str())
            .collect::<Vec<_>>();
        assert!(layer_ids.contains(&"lifecycle"));
        assert!(layer_ids.contains(&"scheduler_cache"));
        assert!(layer_ids.contains(&"host_device"));
        assert!(layer_ids.contains(&"quality"));
        assert!(layer_ids.contains(&"agents"));
        assert!(layer_ids.contains(&"privacy_custody"));
        assert!(payload["summary"]["score"].is_null());
        assert_eq!(
            payload["interpretation"]["performance_thresholds"],
            "user_defined_only"
        );
    }

    #[test]
    fn idle_cue_is_actionable_and_does_not_claim_a_running_model() {
        let app = App::new(Config::default());
        let cue = primary_cue(&app);
        assert_eq!(cue.code, "no_model_running");
        assert_eq!(cue.action_key, "m");
        assert!(cue.detail.contains("Inspect") || cue.detail.contains("available"));
    }
}
