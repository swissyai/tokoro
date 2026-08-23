use super::{
    benchmark_recipes, bloat, commands, connection_model_choices, connection_port, device, eval,
    expand_home, handoff, harness_snippets, huggingface, load_config, monitoring, platform,
    public_model_id, report, save_config, App, Binding, Config,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

const AGENT_SCHEMA: &str = "tokoro.agent.v1";

pub(crate) fn run_if_requested() -> Result<bool, String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(false);
    };
    match command {
        "help" | "--help" | "-h" => {
            print_help();
            Ok(true)
        }
        "commands" => {
            print_commands(args.iter().any(|arg| arg == "--json"))?;
            Ok(true)
        }
        "inspect" => {
            print_inspect(args.iter().any(|arg| arg == "--json"))?;
            Ok(true)
        }
        "monitor" => {
            print_monitor(args.iter().any(|arg| arg == "--json"))?;
            Ok(true)
        }
        "recommendations" => {
            print_recommendations(
                args.iter().any(|arg| arg == "--json"),
                args.iter().any(|arg| arg == "--refresh"),
            )?;
            Ok(true)
        }
        "models" => {
            run_models(&args[1..])?;
            Ok(true)
        }
        "agents" => {
            run_agents(&args[1..])?;
            Ok(true)
        }
        "scan" => {
            run_scan(&args[1..])?;
            Ok(true)
        }
        "config" => {
            run_config(&args[1..])?;
            Ok(true)
        }
        "budget" => {
            run_budget(&args[1..])?;
            Ok(true)
        }
        "benchmark" => {
            run_benchmark(&args[1..])?;
            Ok(true)
        }
        "eval" => {
            run_eval(&args[1..])?;
            Ok(true)
        }
        "integrations" => {
            run_integrations(args.iter().any(|arg| arg == "--json"))?;
            Ok(true)
        }
        "handoff" => {
            run_handoff(&args[1..])?;
            Ok(true)
        }
        "report" => {
            run_report(&args[1..])?;
            Ok(true)
        }
        unknown => Err(format!(
            "unknown command '{unknown}'. Run `tokoro help` for the stable agent interface"
        )),
    }
}

fn print_help() {
    println!(
        r#"tokoro local inference workbench

Interactive:
  tokoro

Agent-native interface:
  tokoro commands --json
  tokoro inspect --json
  tokoro monitor --json
  tokoro recommendations --json [--refresh]
  tokoro models --refresh --json
  tokoro models search "MODEL NAME" --json
  tokoro models download OWNER/REPO [--dir PATH] [--allow-large] [--json]
  tokoro agents --json [--all]
  tokoro agents setup NAME
  tokoro scan --json [--project PATH]
  tokoro scan --deep --json [--project PATH]
  tokoro config show --json
  tokoro config set theme NAME
  tokoro config set density compact|standard|expanded
  tokoro config set default-view home|measure|system|learn|setup|bloat
  tokoro config set onboarding.completed true|false
  tokoro config set observability.focus balanced|latency|throughput|memory|speculation
  tokoro config set observability.history-samples 24..240
  tokoro config set observability.request-retention 8..128
  tokoro budget list --json
  tokoro budget set WORKLOAD METRIC VALUE
  tokoro budget remove WORKLOAD
  tokoro benchmark recipes --json
  tokoro benchmark run RECIPE [--json] [--save]
  tokoro eval list --json
  tokoro eval create NAME --prompt-file PATH [--expected-file PATH]
  tokoro eval review ID pass|fail [--note TEXT]
  tokoro integrations --json
  tokoro handoff list --json
  tokoro handoff prepare BUNDLE TARGET --output DIR [--dry-run] [--replace] [--json]
  tokoro handoff verify DIR --json
  tokoro config set intro.enabled true|false
  tokoro config set intro.motion full|reduced|none
  tokoro config set intro.sound off|tokoro|PATH
  tokoro config set intro.duration-ms 250..5000
  tokoro config set intro.style cursor-threshold|custom
  tokoro config set intro.frames-path PATH
  tokoro config set intro.slogan TEXT
  tokoro config set bloat.scan-project true|false
  tokoro config set bloat.project-dir PATH
  tokoro config panel PANEL on|off
  tokoro report init PATH
  tokoro report history --json
  tokoro report compare BASE CANDIDATE --json
  tokoro report render BUNDLE [--recipe PATH] [--format markdown|json|csv|prometheus|otlp-json] [--output PATH]
  tokoro report verify BUNDLE

Private TUI export saves an immutable bundle plus editable report.toml recipe. Rendering is deterministic for the same checked bundle and recipe. Commands are local. `scan` never opens credential-shaped files. A scan result marked REVIEW cannot be deleted through the agent interface."#
    );
}

fn print_commands(json: bool) -> Result<(), String> {
    let commands = commands::catalog()
        .into_iter()
        .map(|item| {
            serde_json::json!({
                "key": item.key,
                "id": item.action.id(),
                "label": item.label,
                "description": item.detail,
            })
        })
        .collect::<Vec<_>>();
    if json {
        print_json(serde_json::json!({
            "schema": AGENT_SCHEMA,
            "kind": "command_catalog",
            "commands": commands,
        }))
    } else {
        for item in commands::catalog() {
            println!("{:<3} {:<22} {}", item.key, item.label, item.detail);
        }
        Ok(())
    }
}

fn print_inspect(json: bool) -> Result<(), String> {
    let mut cfg = load_config();
    cfg.bloat.scan_project = false;
    let mut app = App::new(cfg);
    app.poll();
    if app.served.is_empty() {
        app.wait_for_runtime(std::time::Duration::from_secs(5));
        app.poll();
    }
    let endpoints = app
        .served
        .iter()
        .map(|server| {
            serde_json::json!({
                "runtime": server.runtime,
                "port": server.port,
                "state": server.state,
                "model": public_model_id(&server.model),
                "mode": server.mode,
                "drafter": server.drafter.as_ref().map(|drafter| public_model_id(drafter)),
            })
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "schema": AGENT_SCHEMA,
        "kind": "inspection",
        "state": if app.online { "live" } else { "idle_baseline" },
        "platform": {
            "os": platform::os_name(),
            "omarchy": platform::is_omarchy(),
            "memory_kind": platform::memory_kind(),
        },
        "runtime": app.engine,
        "port": if app.online { Some(app.port) } else { None },
        "model": if app.online { Some(public_model_id(&app.model)) } else { None },
        "memory": {
            "total_gib": round_one(app.total_mem_gb),
            "used_gib": round_one(app.rss_gb + app.sys_used_gb),
            "available_gib": round_one(app.headroom_gb),
            "server_rss_gib": round_one(app.rss_gb),
            "swap_mib": app.swap_mb.round(),
        },
        "model_storage": {
            "total_gib": round_one(app.device.storage().total_gib),
            "available_gib": round_one(app.device.storage().available_gib),
        },
        "performance": {
            "decode_tokens_per_second": app.real_tg,
            "prefill_tokens_per_second": app.real_pp,
            "runtime_version": app.metrics.runtime_version,
            "draft_acceptance": app.metrics.draft_acceptance,
            "kv_cache_usage": app.metrics.kv_cache_usage,
            "kv_cache_resident_tokens": app.metrics.kv_cache_resident_tokens,
            "kv_cache_evictions": app.metrics.kv_cache_evictions,
            "requests_running": app.metrics.requests_running,
            "requests_waiting": app.metrics.requests_waiting,
            "requests_swapped": app.metrics.requests_swapped,
            "prefix_cache_queries": app.metrics.prefix_queries,
            "prefix_cache_hits": app.metrics.prefix_hits,
            "source": "runtime_report_when_available",
        },
        "context": {
            "current_tokens": app.ceiling.current_tokens,
            "effective_limit_tokens": app.ceiling.effective_max(),
            "binding": match app.ceiling.binding {
                Binding::Model => "model",
                Binding::Memory => "memory",
                Binding::Unknown => "unknown",
            },
        },
        "endpoints": endpoints,
        "agents": app.agents.detected().iter().map(|agent| serde_json::json!({
            "name": agent.display_name,
            "connection": agent.snippet_name,
            "direct": agent.direct,
            "evidence": agent.evidence,
        })).collect::<Vec<_>>(),
        "cues": monitoring::cue_values(&app),
        "monitoring": {
            "profile": monitoring::BASELINE_PROFILE,
            "command": "tokoro monitor --json",
        },
        "next": if app.online { "tokoro agents --json" } else { "tokoro models --refresh --json" },
    });
    if json {
        print_json(payload)
    } else {
        println!(
            "{} | {} | {}",
            if app.online { "LIVE" } else { "IDLE BASELINE" },
            app.engine,
            if app.online {
                public_model_id(&app.model)
            } else {
                "no model loaded".into()
            }
        );
        println!(
            "RAM {:.1}/{:.0} GiB used | {:.1} GiB available | swap {:.0} MiB",
            app.rss_gb + app.sys_used_gb,
            app.total_mem_gb,
            app.headroom_gb,
            app.swap_mb
        );
        println!(
            "models {:.1} GiB disk available | {:.0} GiB total",
            app.device.storage().available_gib,
            app.device.storage().total_gib
        );
        Ok(())
    }
}

fn print_monitor(json: bool) -> Result<(), String> {
    let mut cfg = load_config();
    cfg.bloat.scan_project = false;
    let mut app = App::new(cfg);
    app.poll();
    if app.served.is_empty() {
        app.wait_for_runtime(std::time::Duration::from_secs(5));
        app.poll();
    }
    if json {
        print_json(monitoring::posture_value(&app, AGENT_SCHEMA))
    } else {
        print!("{}", monitoring::posture_text(&app));
        Ok(())
    }
}

fn print_recommendations(json: bool, refresh: bool) -> Result<(), String> {
    let mut cfg = load_config();
    cfg.bloat.scan_project = false;
    let mut app = App::new(cfg);
    app.poll();
    if refresh {
        app.local_ai.refresh(&app.chip, app.total_mem_gb)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        while app.local_ai.loading() && std::time::Instant::now() < deadline {
            if let Some(event) = app.local_ai.poll() {
                if let super::local_ai::Event::Failed(error) = event {
                    return Err(error);
                }
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if app.local_ai.loading() {
            return Err("local.ai refresh timed out after 90 seconds".into());
        }
    }
    let Some(reading) = app.local_ai.reading_for(&app.chip, app.total_mem_gb) else {
        let payload = serde_json::json!({
            "schema": AGENT_SCHEMA,
            "kind": "external_recommendations",
            "state": "unavailable",
            "machine": app.chip,
            "memory_gb": app.total_mem_gb.round(),
            "recommendations": [],
            "source": "public local.ai recommendation",
            "search_adapter": app.local_ai.adapter_label(),
            "next": "tokoro models --refresh --json",
        });
        if json {
            return print_json(payload);
        }
        println!(
            "no cached public local.ai recommendation | search adapter {} | use `tokoro models --refresh --json`",
            app.local_ai.adapter_label()
        );
        return Ok(());
    };
    let recommendations = reading
        .recommendations
        .iter()
        .map(|recommendation| {
            serde_json::json!({
                "label": recommendation.label,
                "model": recommendation.model,
                "intelligence": recommendation.intelligence,
                "tasks_per_hour": recommendation.tasks_per_hour,
                "size_gb": recommendation.size_gb,
                "url": recommendation.url,
                "provenance": if reading.source_method.is_empty() { "cached public-web extraction" } else { &reading.source_method },
            })
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "schema": AGENT_SCHEMA,
        "kind": "external_recommendations",
        "machine": reading.machine,
        "memory_gb": reading.memory_gb,
        "recommendations": recommendations,
        "source": "public local.ai recommendation",
        "source_method": if reading.source_method.is_empty() { "cached public-web extraction" } else { &reading.source_method },
        "source_url": reading.source_url,
        "fetched_unix": reading.fetched_unix,
        "custody": "cached_local",
    });
    if json {
        print_json(payload)
    } else {
        println!(
            "{} | {:.0} GB | local.ai public web",
            reading.machine, reading.memory_gb
        );
        for recommendation in &reading.recommendations {
            println!(
                "{:<16} {} | intelligence {} | speed {} tasks/hr | size {} GB",
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
            );
        }
        Ok(())
    }
}

fn run_models(args: &[String]) -> Result<(), String> {
    let json = args.iter().any(|arg| arg == "--json");
    if args.first().map(String::as_str) == Some("search") {
        let query = args
            .get(1)
            .ok_or_else(|| "models search needs a model name".to_string())?;
        let manifests = huggingface::search_manifests(query)?;
        let models = manifests
            .iter()
            .map(|manifest| {
                serde_json::json!({
                    "repo": manifest.repo,
                    "revision": manifest.revision,
                    "bytes": manifest.total_bytes,
                    "files": manifest.files.len(),
                    "download": format!("tokoro models download {} --json", manifest.repo),
                })
            })
            .collect::<Vec<_>>();
        let payload = serde_json::json!({
            "schema": AGENT_SCHEMA,
            "kind": "huggingface_search",
            "query": query,
            "models": models,
            "checks": ["public", "ungated", "immutable_commit", "safetensors", "lfs_sha256"],
        });
        if json {
            return print_json(payload);
        }
        for manifest in manifests {
            println!(
                "{} | {} | commit {}",
                manifest.repo,
                huggingface::format_bytes(manifest.total_bytes),
                &manifest.revision[..8]
            );
        }
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("download") {
        let repo = args
            .get(1)
            .ok_or_else(|| "models download needs OWNER/REPO".to_string())?;
        let manifest = huggingface::verify_repo(repo)?;
        if manifest.total_bytes > 1024 * 1024 * 1024
            && !args.iter().any(|arg| arg == "--allow-large")
        {
            return Err(format!(
                "{} is {}; add --allow-large after reviewing disk and RAM fit",
                repo,
                huggingface::format_bytes(manifest.total_bytes)
            ));
        }
        let config = load_config();
        let models_dir = args
            .windows(2)
            .find(|pair| pair[0] == "--dir")
            .map(|pair| expand_home(&pair[1]))
            .unwrap_or_else(|| expand_home(&config.server.models_dir));
        let storage = device::Monitor::new(&models_dir).storage();
        let available_bytes = (storage.available_gib * 1024.0 * 1024.0 * 1024.0) as u64;
        if manifest.total_bytes.saturating_add(1024 * 1024 * 1024) > available_bytes {
            return Err(format!(
                "{} needs {} plus 1 GiB free; {:.1} GiB is available",
                repo,
                huggingface::format_bytes(manifest.total_bytes),
                storage.available_gib
            ));
        }
        let mut last_reported = 0;
        let target =
            huggingface::download_manifest(&manifest, &models_dir, |downloaded, total| {
                if !json && downloaded.saturating_sub(last_reported) >= 32 * 1024 * 1024 {
                    eprintln!(
                        "downloaded {} / {}",
                        huggingface::format_bytes(downloaded),
                        huggingface::format_bytes(total)
                    );
                    last_reported = downloaded;
                }
            })?;
        let payload = serde_json::json!({
            "schema": AGENT_SCHEMA,
            "kind": "huggingface_download",
            "repo": manifest.repo,
            "revision": manifest.revision,
            "bytes": manifest.total_bytes,
            "files": manifest.files.len(),
            "checks": ["public", "ungated", "immutable_commit", "safetensors", "lfs_sha256"],
            "installed": target.join("config.json").is_file(),
        });
        if json {
            return print_json(payload);
        }
        println!(
            "installed {} at commit {} | {} | SHA-256 checked",
            manifest.repo,
            &manifest.revision[..8],
            huggingface::format_bytes(manifest.total_bytes)
        );
        return Ok(());
    }

    let refresh = args.iter().any(|arg| arg == "--refresh");
    let models = huggingface::starters()
        .map(|candidate| {
            let manifest = refresh.then(|| huggingface::verify_repo(candidate.repo));
            match manifest {
                Some(Ok(manifest)) => serde_json::json!({
                    "repo": candidate.repo,
                    "label": candidate.label,
                    "purpose": candidate.purpose,
                    "state": "manifest_checked",
                    "revision": manifest.revision,
                    "bytes": manifest.total_bytes,
                    "files": manifest.files.len(),
                    "download": format!("tokoro models download {} --json", candidate.repo),
                }),
                Some(Err(error)) => serde_json::json!({
                    "repo": candidate.repo,
                    "label": candidate.label,
                    "purpose": candidate.purpose,
                    "state": "check_failed",
                    "error": error,
                }),
                None => serde_json::json!({
                    "repo": candidate.repo,
                    "label": candidate.label,
                    "purpose": candidate.purpose,
                    "state": "not_checked",
                    "check": "tokoro models --refresh --json",
                }),
            }
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "schema": AGENT_SCHEMA,
        "kind": "huggingface_catalog",
        "remote_contacted": refresh,
        "models": models,
        "policy": "public ungated repositories; immutable commit; safetensors LFS SHA-256",
    });
    if json {
        print_json(payload)
    } else {
        for model in payload["models"].as_array().into_iter().flatten() {
            println!(
                "{} | {} | {}",
                model["label"].as_str().unwrap_or("model"),
                model["state"].as_str().unwrap_or("unknown"),
                model["repo"].as_str().unwrap_or("unknown")
            );
        }
        Ok(())
    }
}

fn run_agents(args: &[String]) -> Result<(), String> {
    let config = load_config();
    let app = App::new(config);
    let port = connection_port(&app);
    let model = connection_model_choices(&app)
        .into_iter()
        .next()
        .unwrap_or_else(|| "model-id".into());
    let snippets = harness_snippets(&model, port);

    if args.first().map(String::as_str) == Some("setup") {
        let query = args
            .get(1)
            .ok_or_else(|| "agents setup needs an agent name".to_string())?;
        let query_lower = query.to_lowercase();
        let (_, setup) = snippets
            .iter()
            .find(|(name, _)| name.to_lowercase().contains(&query_lower))
            .ok_or_else(|| format!("no setup template named '{query}'"))?;
        println!("{setup}");
        return Ok(());
    }

    let all = args.iter().any(|arg| arg == "--all");
    let rows = snippets
        .iter()
        .filter(|(name, _)| all || app.agents.has(name))
        .map(|(name, setup)| {
            let detection = app
                .agents
                .detected()
                .iter()
                .find(|agent| agent.snippet_name == *name);
            serde_json::json!({
                "name": name,
                "detected": detection.is_some(),
                "direct": detection.map(|agent| agent.direct),
                "evidence": detection.map(|agent| agent.evidence),
                "setup": setup,
            })
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "schema": AGENT_SCHEMA,
        "kind": "agent_connections",
        "endpoint": format!("http://127.0.0.1:{port}/v1"),
        "endpoint_state": if app.online { "live" } else { "configured_not_running" },
        "model": model,
        "agents": rows,
    });
    if args.iter().any(|arg| arg == "--json") {
        print_json(payload)
    } else {
        for agent in payload["agents"].as_array().into_iter().flatten() {
            println!(
                "{} | {}",
                agent["name"].as_str().unwrap_or("agent"),
                if agent["detected"].as_bool() == Some(true) {
                    "detected"
                } else {
                    "not detected"
                }
            );
        }
        Ok(())
    }
}

fn run_scan(args: &[String]) -> Result<(), String> {
    let json = args.iter().any(|arg| arg == "--json");
    let deep = args.iter().any(|arg| arg == "--deep");
    let project = args
        .windows(2)
        .find(|pair| pair[0] == "--project")
        .map(|pair| PathBuf::from(&pair[1]))
        .unwrap_or_else(|| configured_project_root(&load_config()));
    let report = if deep {
        bloat::deep_scan(&project)
    } else {
        bloat::quick_scan(&project)
    };
    let findings = report
        .findings
        .iter()
        .map(|finding| {
            serde_json::json!({
                "code": finding.code,
                "classification": finding.disposition.label(),
                "confidence": finding.confidence.label(),
                "title": finding.title,
                "evidence": finding.evidence,
                "next": finding.action,
                "reclaim_bytes": finding.reclaim_bytes,
                "artifact": finding.relative_path().map(|path| path.to_string_lossy()),
                "agent_can_remove": false,
            })
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "schema": AGENT_SCHEMA,
        "kind": "bloat_scan",
        "scope": "selected_project",
        "depth": report.depth.label(),
        "files": report.scanned_files,
        "directories": report.scanned_directories,
        "bounded": report.truncated,
        "findings": findings,
        "custody": "local_only",
    });
    if json {
        print_json(payload)
    } else {
        println!(
            "{} findings | {} files | {} directories{}",
            report.findings.len(),
            report.scanned_files,
            report.scanned_directories,
            if report.truncated { " | bounded" } else { "" }
        );
        for finding in &report.findings {
            println!(
                "[{}] {}\n    {}",
                finding.disposition.label(),
                finding.title,
                finding.evidence
            );
        }
        Ok(())
    }
}

fn run_config(args: &[String]) -> Result<(), String> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err("config needs show, set, or panel".into());
    };
    let mut cfg = load_config();
    match action {
        "show" => {
            let payload = serde_json::json!({
                "schema": AGENT_SCHEMA,
                "kind": "configuration",
                "theme": if cfg.theme.name.is_empty() { "auto" } else { &cfg.theme.name },
                "layout": {
                    "density": cfg.layout.density,
                    "default_view": cfg.layout.default_view,
                    "panels": cfg.layout.panels,
                    "hidden_panels": cfg.layout.hidden_panels,
                },
                "telemetry": { "ports": cfg.telemetry.ports },
                "observability": {
                    "focus": cfg.observability.focus(),
                    "history_samples": cfg.observability.history_samples(),
                    "request_retention": cfg.observability.request_retention(),
                    "custody": "session_metrics_only",
                },
                "benchmark": {
                    "prompt_tokens": cfg.benchmark.prompt_tokens,
                    "output_limit_tokens": cfg.benchmark.gen_tokens,
                    "runs": cfg.benchmark.runs,
                    "prompt_sweep": cfg.benchmark.sweep,
                    "concurrency_sweep": cfg.benchmark.concurrency_levels(),
                    "budgeted_workloads": cfg.benchmark.budgets.len(),
                },
                "server": { "port": cfg.server.port },
                "onboarding": {
                    "completed": cfg.onboarding.completed,
                    "custody": "local_config_only",
                },
                "intro": {
                    "enabled": cfg.intro.enabled,
                    "style": cfg.intro.style,
                    "duration_ms": cfg.intro.duration_ms,
                    "sound": if matches!(cfg.intro.sound.as_str(), "" | "off" | "tokoro" | "freedom") {
                        cfg.intro.sound.as_str()
                    } else {
                        "<custom>"
                    },
                    "motion": cfg.intro.motion,
                    "slogan": if cfg.intro.slogan.is_empty() { "" } else { "<custom>" },
                    "frames_path": if cfg.intro.frames_path.is_empty() { "" } else { "<custom>" },
                },
                "bloat": {
                    "scan_project": cfg.bloat.scan_project,
                    "project_dir": redact_config_path(&cfg.bloat.project_dir),
                },
            });
            if args.iter().any(|arg| arg == "--json") {
                print_json(payload)
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?
                );
                Ok(())
            }
        }
        "set" => {
            let key = args
                .get(1)
                .ok_or_else(|| "config set needs a key".to_string())?;
            let value = args
                .get(2)
                .ok_or_else(|| "config set needs a value".to_string())?;
            match key.as_str() {
                "theme" => {
                    cfg.theme.name = if value == "auto" {
                        String::new()
                    } else {
                        value.clone()
                    }
                }
                "density" if matches!(value.as_str(), "compact" | "standard" | "expanded") => {
                    cfg.layout.density = value.clone();
                }
                "default-view"
                    if matches!(
                        value.as_str(),
                        "home" | "measure" | "system" | "learn" | "setup" | "bloat"
                    ) =>
                {
                    cfg.layout.default_view = if value == "setup" {
                        "customize".into()
                    } else {
                        value.clone()
                    };
                }
                "observability.focus"
                    if matches!(
                        value.as_str(),
                        "balanced" | "latency" | "throughput" | "memory" | "speculation"
                    ) =>
                {
                    cfg.observability.focus = value.clone();
                }
                "observability.history-samples" => {
                    let samples = value
                        .parse::<usize>()
                        .map_err(|_| "observability.history-samples must be an integer from 24 to 240")?;
                    if !(24..=240).contains(&samples) {
                        return Err("observability.history-samples must be from 24 to 240".into());
                    }
                    cfg.observability.history_samples = samples;
                }
                "observability.request-retention" => {
                    let records = value
                        .parse::<usize>()
                        .map_err(|_| "observability.request-retention must be an integer from 8 to 128")?;
                    if !(8..=128).contains(&records) {
                        return Err("observability.request-retention must be from 8 to 128".into());
                    }
                    cfg.observability.request_retention = records;
                }
                "onboarding.completed" if matches!(value.as_str(), "true" | "false") => {
                    cfg.onboarding.completed = value == "true";
                }
                "intro.enabled" if matches!(value.as_str(), "true" | "false") => {
                    cfg.intro.enabled = value == "true";
                }
                "intro.motion" if matches!(value.as_str(), "full" | "reduced" | "none") => {
                    cfg.intro.motion = value.clone();
                }
                "intro.sound" if !value.trim().is_empty() => cfg.intro.sound = value.clone(),
                "intro.duration-ms" => {
                    let duration = value
                        .parse::<u64>()
                        .map_err(|_| "intro.duration-ms must be an integer from 250 to 5000")?;
                    if !(250..=5_000).contains(&duration) {
                        return Err("intro.duration-ms must be from 250 to 5000".into());
                    }
                    cfg.intro.duration_ms = duration;
                }
                "intro.style" if matches!(value.as_str(), "cursor-threshold" | "custom") => {
                    cfg.intro.style = value.clone();
                }
                "intro.frames-path" => cfg.intro.frames_path = value.clone(),
                "intro.slogan" => cfg.intro.slogan = value.clone(),
                "bloat.scan-project" if matches!(value.as_str(), "true" | "false") => {
                    cfg.bloat.scan_project = value == "true";
                }
                "bloat.project-dir" => cfg.bloat.project_dir = value.clone(),
                "density" => return Err("density must be compact, standard, or expanded".into()),
                "default-view" => {
                    return Err(
                        "default-view must be home, measure, system, learn, setup, or bloat".into(),
                    )
                }
                "observability.focus" => {
                    return Err("observability.focus must be balanced, latency, throughput, memory, or speculation".into())
                }
                "onboarding.completed" => {
                    return Err("onboarding.completed must be true or false".into())
                }
                "intro.enabled" => return Err("intro.enabled must be true or false".into()),
                "intro.motion" => return Err("intro.motion must be full, reduced, or none".into()),
                "intro.sound" => return Err("intro.sound must not be empty".into()),
                "intro.style" => {
                    return Err("intro.style must be cursor-threshold or custom".into())
                }
                "bloat.scan-project" => {
                    return Err("bloat.scan-project must be true or false".into())
                }
                _ => return Err(format!("unsupported config key '{key}'")),
            }
            save_config(&cfg)?;
            println!("saved {key}");
            Ok(())
        }
        "panel" => {
            let panel = args
                .get(1)
                .ok_or_else(|| "config panel needs a panel name".to_string())?;
            let state = args
                .get(2)
                .ok_or_else(|| "config panel needs on or off".to_string())?;
            if !cfg.layout.panels.contains(panel) {
                return Err(format!("unknown panel '{panel}'"));
            }
            match state.as_str() {
                "on" => cfg.layout.hidden_panels.retain(|hidden| hidden != panel),
                "off" if !cfg.layout.hidden_panels.contains(panel) => {
                    cfg.layout.hidden_panels.push(panel.clone());
                }
                "off" => {}
                _ => return Err("panel state must be on or off".into()),
            }
            save_config(&cfg)?;
            println!("panel {panel} {state}");
            Ok(())
        }
        _ => Err("config needs show, set, or panel".into()),
    }
}

fn run_budget(args: &[String]) -> Result<(), String> {
    let action = args.first().map(String::as_str).unwrap_or("list");
    let mut cfg = load_config();
    match action {
        "list" => {
            if args.iter().any(|arg| arg == "--json") {
                print_json(serde_json::json!({
                    "schema": AGENT_SCHEMA,
                    "kind": "workload_budgets",
                    "budgets": cfg.benchmark.budgets,
                    "policy": "user_defined_no_vendor_defaults",
                }))
            } else if cfg.benchmark.budgets.is_empty() {
                println!("no workload budgets configured");
                Ok(())
            } else {
                for budget in &cfg.benchmark.budgets {
                    println!(
                        "{} | TTFT p95 {:?} ms | TPOT p95 {:?} ms/token | E2E p95 {:?} ms | decode {:?} tok/s | system {:?} tok/s | RSS {:?} GiB | swap {:?} MiB | waiting {:?}",
                        budget.workload,
                        budget.max_ttft_p95_ms,
                        budget.max_tpot_p95_ms,
                        budget.max_end_to_end_p95_ms,
                        budget.min_decode_tokens_per_second,
                        budget.min_system_tokens_per_second,
                        budget.max_server_rss_gib,
                        budget.max_swap_mib,
                        budget.max_waiting_requests
                    );
                }
                Ok(())
            }
        }
        "set" => {
            let workload = args
                .get(1)
                .ok_or_else(|| "budget set needs a quoted workload name".to_string())?;
            let metric = args.get(2).ok_or_else(|| {
                "budget set needs ttft-p95-ms, tpot-p95-ms, e2e-p95-ms, decode-tps, system-tps, rss-gib, swap-mib, or waiting"
                    .to_string()
            })?;
            let raw = args
                .get(3)
                .ok_or_else(|| "budget set needs a numeric value".to_string())?;
            let value = raw
                .parse::<f64>()
                .map_err(|_| "budget value must be a non-negative number".to_string())?;
            if !value.is_finite() || value < 0.0 {
                return Err("budget value must be a non-negative finite number".into());
            }
            let index = cfg
                .benchmark
                .budgets
                .iter()
                .position(|budget| budget.workload.eq_ignore_ascii_case(workload))
                .unwrap_or_else(|| {
                    cfg.benchmark.budgets.push(Default::default());
                    cfg.benchmark.budgets.len() - 1
                });
            let budget = &mut cfg.benchmark.budgets[index];
            budget.workload = workload.clone();
            match metric.as_str() {
                "ttft-p95-ms" => budget.max_ttft_p95_ms = Some(value),
                "tpot-p95-ms" => budget.max_tpot_p95_ms = Some(value),
                "e2e-p95-ms" => budget.max_end_to_end_p95_ms = Some(value),
                "decode-tps" => budget.min_decode_tokens_per_second = Some(value),
                "system-tps" => budget.min_system_tokens_per_second = Some(value),
                "rss-gib" => budget.max_server_rss_gib = Some(value),
                "swap-mib" => budget.max_swap_mib = Some(value),
                "waiting" if value.fract() == 0.0 && value <= u64::MAX as f64 => {
                    budget.max_waiting_requests = Some(value as u64)
                }
                "waiting" => return Err("waiting budget must be a whole request count".into()),
                _ => {
                    return Err("budget metric must be ttft-p95-ms, tpot-p95-ms, e2e-p95-ms, decode-tps, system-tps, rss-gib, swap-mib, or waiting".into())
                }
            }
            save_config(&cfg)?;
            println!("saved {metric} budget for {workload}");
            Ok(())
        }
        "remove" => {
            let workload = args
                .get(1)
                .ok_or_else(|| "budget remove needs a quoted workload name".to_string())?;
            let before = cfg.benchmark.budgets.len();
            cfg.benchmark
                .budgets
                .retain(|budget| !budget.workload.eq_ignore_ascii_case(workload));
            if cfg.benchmark.budgets.len() == before {
                return Err(format!("no budget configured for '{workload}'"));
            }
            save_config(&cfg)?;
            println!("removed budget for {workload}");
            Ok(())
        }
        _ => Err("budget needs list, set, or remove".into()),
    }
}

fn run_benchmark(args: &[String]) -> Result<(), String> {
    let action = args.first().map(String::as_str).unwrap_or("recipes");
    let mut cfg = load_config();
    cfg.bloat.scan_project = false;
    let mut app = App::new(cfg);
    if action == "recipes" {
        let recipes = benchmark_recipes(&app)
            .into_iter()
            .map(|recipe| {
                serde_json::json!({
                    "name": recipe.name,
                    "description": recipe.description,
                    "prompt_tokens": recipe.prompt_tokens,
                    "output_limit_tokens": recipe.gen_tokens,
                    "runs": recipe.runs,
                    "prompt_sweep": recipe.sweep_sizes,
                    "concurrency_sweep": recipe.concurrency_levels,
                })
            })
            .collect::<Vec<_>>();
        if args.iter().any(|arg| arg == "--json") {
            return print_json(serde_json::json!({
                "schema": AGENT_SCHEMA,
                "kind": "benchmark_recipes",
                "recipes": recipes,
            }));
        }
        for recipe in recipes {
            println!(
                "{} | {}",
                recipe["name"].as_str().unwrap_or("recipe"),
                recipe["description"].as_str().unwrap_or("")
            );
        }
        return Ok(());
    }
    if action != "run" {
        return Err("benchmark needs recipes or run".into());
    }
    let query = args
        .get(1)
        .ok_or_else(|| "benchmark run needs a recipe name".to_string())?
        .to_lowercase();
    let recipe = benchmark_recipes(&app)
        .into_iter()
        .find(|recipe| recipe.name.to_lowercase() == query)
        .or_else(|| {
            benchmark_recipes(&app)
                .into_iter()
                .find(|recipe| recipe.name.to_lowercase().contains(&query))
        })
        .ok_or_else(|| format!("no benchmark recipe matches '{query}'"))?;

    app.poll();
    if !app.online {
        app.wait_for_runtime(std::time::Duration::from_secs(5));
        app.poll();
    }
    if !app.online {
        return Err("no responding local OpenAI-compatible endpoint".into());
    }
    app.start_benchmark_plan(recipe);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1_200);
    let mut next_poll = std::time::Instant::now();
    while app.bench.active && std::time::Instant::now() < deadline {
        app.bench_tick();
        if std::time::Instant::now() >= next_poll {
            app.poll();
            next_poll = std::time::Instant::now() + std::time::Duration::from_millis(250);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if app.bench.active {
        return Err("benchmark timed out after 20 minutes".into());
    }
    if app.bench.results.is_empty()
        && app.bench.sweep_results.is_empty()
        && app.bench.concurrency_results.is_empty()
    {
        return Err(app
            .bench
            .summary
            .clone()
            .unwrap_or_else(|| "benchmark produced no valid measurements".into()));
    }
    if args.iter().any(|arg| arg == "--save") {
        let id = report::save_private_report(&app)?;
        eprintln!("saved checked report {id}");
    }
    if args.iter().any(|arg| arg == "--json") {
        let envelope = report::capture(&app)?;
        let value = serde_json::from_str(&report::render_json(&envelope)?)
            .map_err(|error| error.to_string())?;
        print_json(value)
    } else {
        println!(
            "{}",
            app.bench.summary.as_deref().unwrap_or("benchmark complete")
        );
        Ok(())
    }
}

fn run_eval(args: &[String]) -> Result<(), String> {
    let action = args.first().map(String::as_str).unwrap_or("list");
    match action {
        "list" => {
            let index = eval::list()?;
            if args.iter().any(|arg| arg == "--json") {
                print_json(serde_json::json!({
                    "schema": AGENT_SCHEMA,
                    "kind": "local_eval_fixtures",
                    "fixtures": index,
                    "custody": "local_private",
                }))
            } else {
                for fixture in index {
                    println!(
                        "{} | {} | {} | prompt {} | expected {}",
                        fixture.id,
                        fixture.label,
                        fixture.status,
                        if fixture.has_prompt { "yes" } else { "no" },
                        if fixture.has_expected { "yes" } else { "no" }
                    );
                }
                Ok(())
            }
        }
        "create" => {
            let name = args
                .get(1)
                .ok_or_else(|| "eval create needs a name".to_string())?;
            let prompt = flag_value(args, "--prompt-file").map(PathBuf::from);
            let expected = flag_value(args, "--expected-file").map(PathBuf::from);
            let id = eval::create_manual(name, prompt.as_deref(), expected.as_deref())?;
            println!("created private eval fixture {id}");
            Ok(())
        }
        "review" => {
            let id = args
                .get(1)
                .ok_or_else(|| "eval review needs a fixture id".to_string())?;
            let status = args
                .get(2)
                .ok_or_else(|| "eval review needs pass or fail".to_string())?;
            let note = flag_value(args, "--note").unwrap_or("");
            eval::review(id, status, note)?;
            println!("reviewed {id}: {status}");
            Ok(())
        }
        "show" => {
            let id = args
                .get(1)
                .ok_or_else(|| "eval show needs a fixture id".to_string())?;
            let value = eval::show(id)?;
            if args.iter().any(|arg| arg == "--json") {
                print_json(value)
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?
                );
                Ok(())
            }
        }
        _ => Err("eval needs list, create, show, or review".into()),
    }
}

fn run_integrations(json: bool) -> Result<(), String> {
    let mut config = load_config();
    config.bloat.scan_project = false;
    let mut app = App::new(config);
    app.poll();
    if app.served.is_empty() {
        app.wait_for_runtime(std::time::Duration::from_secs(2));
    }
    let port = connection_port(&app);
    let model = connection_model_choices(&app)
        .into_iter()
        .next()
        .unwrap_or_else(|| "model-id".into());
    let clients = harness_snippets(&model, port)
        .iter()
        .map(|(name, _)| {
            serde_json::json!({
                "name": name,
                "detected": app.agents.has(name),
                "connection": "openai_compatible_local_endpoint",
                "prepare": ["tokoro", "agents", "setup", name],
                "uploads": false,
            })
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "schema": AGENT_SCHEMA,
        "kind": "integration_catalog",
        "endpoint": {
            "url": format!("http://127.0.0.1:{port}/v1"),
            "state": if app.online { "live" } else { "configured_not_running" },
            "model": model,
            "clients": clients,
        },
        "report_handoffs": handoff::targets(),
        "policy": {
            "uploads": "never_automatic",
            "credentials": "not_collected_for_handoffs",
            "verification": "tokoro.handoff.v1_sha256_manifest",
        },
    });
    if json {
        print_json(payload)
    } else {
        println!(
            "{} local clients | {} checked report handoffs | no automatic uploads",
            payload["endpoint"]["clients"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0),
            payload["report_handoffs"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0)
        );
        println!("use `tokoro integrations --json` for commands and custody details");
        Ok(())
    }
}

fn run_handoff(args: &[String]) -> Result<(), String> {
    let action = args.first().map(String::as_str).unwrap_or("list");
    match action {
        "list" => {
            let targets = handoff::targets();
            if args.iter().any(|arg| arg == "--json") {
                print_json(serde_json::json!({
                    "schema": AGENT_SCHEMA,
                    "kind": "handoff_targets",
                    "targets": targets,
                    "upload_policy": "prepare_local_files_only",
                }))
            } else {
                for target in targets {
                    println!("{:<12} {}", target.id, target.purpose);
                }
                Ok(())
            }
        }
        "prepare" => {
            let bundle = args.get(1).ok_or_else(|| {
                "handoff prepare needs a bundle path or checked report id".to_string()
            })?;
            let target = args
                .get(2)
                .ok_or_else(|| "handoff prepare needs a target".to_string())?;
            let output = flag_value(args, "--output")
                .map(PathBuf::from)
                .ok_or_else(|| "handoff prepare needs --output DIR".to_string())?;
            let plan = handoff::prepare(
                bundle,
                target,
                &output,
                args.iter().any(|arg| arg == "--replace"),
                args.iter().any(|arg| arg == "--dry-run"),
            )?;
            if args.iter().any(|arg| arg == "--json") {
                print_json(serde_json::json!({
                    "schema": AGENT_SCHEMA,
                    "kind": if plan.dry_run { "handoff_plan" } else { "handoff_prepared" },
                    "handoff": plan,
                }))
            } else {
                println!(
                    "{} handoff {} | {} files | upload not performed",
                    if plan.dry_run { "planned" } else { "prepared" },
                    plan.target,
                    plan.files.len()
                );
                Ok(())
            }
        }
        "verify" => {
            let directory = args
                .get(1)
                .map(PathBuf::from)
                .ok_or_else(|| "handoff verify needs a directory".to_string())?;
            let receipt = handoff::verify(&directory)?;
            if args.iter().any(|arg| arg == "--json") {
                print_json(serde_json::json!({
                    "schema": AGENT_SCHEMA,
                    "kind": "handoff_verification",
                    "receipt": receipt,
                }))
            } else {
                println!(
                    "verified {} handoff | {} files | bundle {}",
                    receipt.target,
                    receipt.files_verified,
                    &receipt.bundle_sha256[..12]
                );
                Ok(())
            }
        }
        _ => Err("handoff needs list, prepare, or verify".into()),
    }
}

fn run_report(args: &[String]) -> Result<(), String> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err("report needs init, history, compare, render, or verify".into());
    };
    match action {
        "init" => {
            let path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("tokoro-report.toml"));
            report::write_default_recipe(&path)?;
            println!("wrote editable report recipe {}", path.display());
            Ok(())
        }
        "history" => {
            let history = report::saved_history()?;
            if args.iter().any(|arg| arg == "--json") {
                print_json(serde_json::json!({
                    "schema": AGENT_SCHEMA,
                    "kind": "checked_report_history",
                    "entries": history.entries,
                    "rejected": history.rejected,
                    "custody": "local_checked_bundles",
                }))
            } else {
                for entry in history.entries {
                    println!(
                        "{} | {} | {} | {} {} | {} | env {} | workload {} | {} runs | {} concurrency points | {} budget breaches",
                        entry.id,
                        entry.captured_unix,
                        entry.model,
                        entry.engine,
                        entry.engine_version,
                        entry.workload,
                        entry.environment_id,
                        entry.workload_id,
                        entry.runs,
                        entry.concurrency_points,
                        entry.budget_breaches
                    );
                }
                for rejected in history.rejected {
                    eprintln!("rejected {rejected}");
                }
                Ok(())
            }
        }
        "compare" => {
            let baseline = args
                .get(1)
                .ok_or_else(|| "report compare needs a baseline id or bundle path".to_string())?;
            let candidate = args
                .get(2)
                .ok_or_else(|| "report compare needs a candidate id or bundle path".to_string())?;
            let comparison = report::compare_saved(baseline, candidate)?;
            if args.iter().any(|arg| arg == "--json") {
                print_json(serde_json::json!({
                    "schema": AGENT_SCHEMA,
                    "kind": "report_comparison",
                    "comparison": comparison,
                }))
            } else {
                println!(
                    "{} -> {} | {}",
                    comparison.baseline,
                    comparison.candidate,
                    if comparison.comparable {
                        "comparable"
                    } else {
                        "not comparable"
                    }
                );
                for blocker in comparison.blockers {
                    println!("blocked: {blocker}");
                }
                for warning in comparison.warnings {
                    println!("warning: {warning}");
                }
                for change in comparison.configuration_changes {
                    println!("changed: {change}");
                }
                for delta in comparison.deltas {
                    println!(
                        "{} | {:.1} -> {:.1} {} | {:+.1}{}",
                        delta.metric,
                        delta.baseline,
                        delta.candidate,
                        delta.unit,
                        delta.absolute,
                        delta
                            .percent
                            .map(|value| format!(" | {value:+.1}%"))
                            .unwrap_or_default()
                    );
                }
                Ok(())
            }
        }
        "render" => {
            let bundle = args
                .get(1)
                .map(PathBuf::from)
                .ok_or_else(|| "report render needs a bundle.json path".to_string())?;
            let recipe = flag_value(args, "--recipe").map(PathBuf::from);
            let format = flag_value(args, "--format").unwrap_or("markdown");
            let rendered = report::render_saved(&bundle, recipe.as_deref(), format)?;
            if let Some(output) = flag_value(args, "--output") {
                let output = PathBuf::from(output);
                if let Some(parent) = output.parent() {
                    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                fs::write(&output, rendered).map_err(|error| error.to_string())?;
                println!("wrote {}", output.display());
            } else {
                print!("{rendered}");
            }
            Ok(())
        }
        "verify" => {
            let bundle = args
                .get(1)
                .map(PathBuf::from)
                .ok_or_else(|| "report verify needs a bundle.json path".to_string())?;
            let rendered = report::render_saved(&bundle, None, "json")?;
            let envelope = serde_json::from_str::<report::ReportEnvelope>(&rendered)
                .map_err(|error| error.to_string())?;
            println!("verified {} {}", envelope.schema, &envelope.sha256[..12]);
            Ok(())
        }
        _ => Err("report needs init, history, compare, render, or verify".into()),
    }
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn configured_project_root(cfg: &Config) -> PathBuf {
    let configured = expand_home(&cfg.bloat.project_dir);
    if configured.is_absolute() {
        configured
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(configured)
    }
}

fn redact_config_path(path: &str) -> String {
    if Path::new(path).is_absolute() {
        "<configured-project>".into()
    } else {
        path.into()
    }
}

fn print_json(value: serde_json::Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn round_one(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}
