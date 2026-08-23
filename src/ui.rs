use super::{
    benchmark_recipes, bloat, commands, connection_description, connection_matches,
    connection_model_choices, connection_port, harness_snippets, huggingface, learn, monitoring,
    platform, public_model_id, report, theme_matches, App, Binding, ExpandedPane, FocusPanel,
    ModelTab, Popup, Screen, Stage, Theme, BYTES_PER_GIB, ONBOARDING_CHOICES,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::{collections::VecDeque, time::Duration};

fn fmt_dur(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

fn bar(frac: f64, w: usize) -> String {
    let n = (frac.clamp(0.0, 1.0) * w as f64) as usize;
    "#".repeat(n) + &"-".repeat(w.saturating_sub(n))
}

fn sparkline(values: &VecDeque<f64>, width: usize, scale: f64, renderer: &str) -> String {
    const ASCII: &[char] = &[' ', '.', ':', '-', '=', '+', '*', '#', '@'];
    const UNICODE: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    const BLOCKS: &[char] = &[' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
    let levels = match renderer {
        "ascii" => ASCII,
        "blocks" => BLOCKS,
        _ => UNICODE,
    };
    let start = values.len().saturating_sub(width);
    values
        .iter()
        .skip(start)
        .map(|value| {
            let level = ((value / scale.max(1.0)) * (levels.len() - 1) as f64)
                .round()
                .clamp(0.0, (levels.len() - 1) as f64) as usize;
            levels[level]
        })
        .collect()
}

fn sample_stats(values: &VecDeque<f64>) -> (usize, f64, f64, f64) {
    let samples = values
        .iter()
        .copied()
        .filter(|value| *value > 0.0)
        .collect::<Vec<_>>();
    if samples.is_empty() {
        return (0, 0.0, 0.0, 0.0);
    }
    let minimum = samples.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = samples.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    (samples.len(), minimum, mean, maximum)
}

fn percentile(values: impl Iterator<Item = f64>, percentile: f64) -> Option<f64> {
    let mut values = values.filter(|value| value.is_finite()).collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * percentile.clamp(0.0, 1.0)).round() as usize;
    values.get(index).copied()
}

// ─────────────────────────────── UI ───────────────────────────────

fn panel_label(name: &str) -> &'static str {
    match name {
        "sources" => "endpoints and provenance",
        "memory" => "memory stack",
        "performance" => "performance and speculation",
        "stages" => "inference path",
        "history" => "request history",
        "interference" => "system pressure",
        "streams" => "inference signals",
        "bloat" => "bloat check",
        _ => "unknown panel",
    }
}

fn screen_name(screen: Screen) -> &'static str {
    match screen {
        Screen::Home => "overview",
        Screen::Measure => "measure",
        Screen::System => "system",
        Screen::Learn => "learn",
        Screen::Customize => "setup",
        Screen::Bloat => "bloat",
    }
}

fn panel_block<'a>(title: &'a str, t: &'a Theme) -> Block<'a> {
    Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.dim))
}

fn focus_panel_title(panel: FocusPanel, app: &App) -> &'static str {
    match panel {
        FocusPanel::HomeModel if app.online => "RUNNING",
        FocusPanel::HomeModel => "MODEL",
        FocusPanel::HomeCapacity => "CAPACITY",
        FocusPanel::HomeSources => "INVENTORY",
        FocusPanel::HomeNext => "NEXT",
        FocusPanel::Performance => "PERFORMANCE / SPECULATION",
        FocusPanel::Streams => "INFERENCE SIGNALS",
        FocusPanel::Stages => "INFERENCE PATH",
        FocusPanel::History => "REQUEST HISTORY",
        FocusPanel::Memory => "MEMORY STACK",
        FocusPanel::Pressure => "SYSTEM PRESSURE",
        FocusPanel::Bloat => "BLOAT CHECK",
        FocusPanel::Sources => "ENDPOINTS / PROVENANCE",
    }
}

fn render_panel_ring(f: &mut Frame, area: Rect, app: &App, panel: FocusPanel, expanded: bool) {
    let selected = if expanded {
        app.expanded_pane == ExpandedPane::Content
    } else {
        app.selected_panel() == Some(panel)
    };
    if !selected {
        return;
    }
    let hint = if expanded {
        "1/2 | Tab details".to_string()
    } else {
        app.selected_panel_position()
            .map(|(position, total)| format!("{position}/{total} | Enter open"))
            .unwrap_or_else(|| "Enter open".into())
    };
    let title = if expanded {
        format!(
            " {} / FULL EVIDENCE [FOCUSED] ",
            focus_panel_title(panel, app)
        )
    } else {
        format!(" {} [FOCUSED] ", focus_panel_title(panel, app))
    };
    let block = Block::default()
        .title(title)
        .title_bottom(
            Line::from(format!(" {hint} "))
                .style(Style::default().fg(app.theme.accent))
                .right_aligned(),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.accent));
    f.render_widget(block, area);
}

fn render_panel_content(f: &mut Frame, area: Rect, app: &App, panel: FocusPanel) {
    match panel {
        FocusPanel::HomeModel => render_focus(f, area, app),
        FocusPanel::HomeCapacity => render_home_signals(f, area, app),
        FocusPanel::HomeSources => render_home_sources(f, area, app),
        FocusPanel::HomeNext => render_home_next(f, area, app),
        FocusPanel::Performance => render_performance(f, area, app, &app.theme),
        FocusPanel::Streams => render_streams(f, area, app, &app.theme),
        FocusPanel::Stages => render_stages(f, area, app, &app.theme),
        FocusPanel::History => render_history(f, area, app, &app.theme),
        FocusPanel::Memory => render_memory(f, area, app, &app.theme),
        FocusPanel::Pressure => render_interference(f, area, app, &app.theme),
        FocusPanel::Bloat => render_bloat(f, area, app),
        FocusPanel::Sources => render_sources(f, area, app),
    }
}

fn panel_guide_lines(app: &App, panel: FocusPanel) -> Vec<Line<'static>> {
    let t = &app.theme;
    let heading = |value: &str| {
        Line::from(Span::styled(
            value.to_string(),
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        ))
    };
    let action = |key: &str, value: &str| {
        Line::from(vec![
            Span::styled(
                format!("{key:<3}"),
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(value.to_string(), Style::default().fg(t.fg)),
        ])
    };
    let mut lines = match panel {
        FocusPanel::HomeModel => vec![
            heading("WHAT THIS SHOWS"),
            Line::from("The responding runtime and model. Installed is not loaded."),
            Line::from(""),
            heading("ACTIONS"),
            action("m", "choose a model"),
            action("s", "start or stop managed serving"),
            action("c", "prepare agent setup"),
        ],
        FocusPanel::HomeCapacity => vec![
            heading("WHAT THIS SHOWS"),
            Line::from("Host RAM and model-disk space are separate limits."),
            Line::from("Device memory is separate on Linux and Windows discrete GPUs."),
            Line::from(""),
            heading("NEXT"),
            action("3", "open full system evidence"),
            action("m", "compare local model sizes"),
        ],
        FocusPanel::HomeSources => vec![
            heading("WHAT THIS SHOWS"),
            Line::from("Loaded endpoints, installed inventory, and checked artifacts."),
            Line::from("An installed model stays inactive until a runtime reports it loaded."),
            Line::from(""),
            heading("ACTIONS"),
            action("m", "local model targets"),
            action("h", "checked Hugging Face artifacts"),
            action("l", "public comparison evidence"),
        ],
        FocusPanel::HomeNext => vec![
            heading("START HERE"),
            Line::from("Choose one action. Nothing is uploaded or started silently."),
            Line::from(""),
            heading("SHORTCUTS"),
            action("m", "load a model"),
            action("b", "measure the loaded model"),
            action("c", "connect a detected agent"),
            action("/", "search every command"),
        ],
        FocusPanel::Performance => vec![
            heading("READING ORDER"),
            Line::from("TTFT is the wait. Decode is response speed. Prefill is prompt reading."),
            Line::from("Live request values win over cumulative runtime values."),
            Line::from(""),
            heading("ACTIONS"),
            action("b", "quick deterministic benchmark"),
            action("r", "choose a workload"),
            action("B", "sweep prompt sizes"),
            action("?", "explain each metric"),
        ],
        FocusPanel::Streams => vec![
            heading("WHAT THIS SHOWS"),
            Line::from("Decode, prefill, TTFT, KV use, queue, acceptance, and engine load."),
            Line::from("Compact focus is configurable; expanded evidence keeps every signal."),
            Line::from(""),
            heading("ACTIONS"),
            action("b", "generate a repeatable sample"),
            action("r", "choose a longer workload"),
            action("5", "change focus and retention"),
        ],
        FocusPanel::Stages => vec![
            heading("WHAT THIS SHOWS"),
            Line::from("One request from prompt and cache reuse through first token and decode."),
            Line::from(
                "Unknown queue or verification stages stay unavailable rather than estimated.",
            ),
            Line::from(""),
            heading("ACTIONS"),
            action("b", "run a request"),
            action("r", "choose workload shape"),
            action("e", "save selected metrics as a private eval fixture"),
        ],
        FocusPanel::History => vec![
            heading("WHAT THIS SHOWS"),
            Line::from(format!(
                "{} of {} session records retained.",
                app.spans.len(),
                app.cfg.observability.request_retention()
            )),
            Line::from("Prompts and responses are not shown or persisted here."),
            Line::from(""),
            heading("ACTIONS"),
            action("b", "add a benchmark request"),
            action("e", "create a human-reviewed local eval fixture"),
            action("p", "preview a redacted export"),
        ],
        FocusPanel::Memory => vec![
            heading("WHAT THIS SHOWS"),
            Line::from("Host RAM, server RSS, device memory, swap, and headroom."),
            Line::from("Discrete GPU memory is never added inside process RSS."),
            Line::from(""),
            heading("ACTIONS"),
            action("m", "compare model footprint"),
            action("B", "test context growth"),
        ],
        FocusPanel::Pressure => vec![
            heading("WHAT THIS SHOWS"),
            Line::from("Processes outside the inference server that can distort results."),
            Line::from(""),
            heading("ACTIONS"),
            action("j/k", "select a process"),
            action("x x", "confirm termination"),
            Line::from(Span::styled(
                "Tokoro and terminal processes are protected.",
                Style::default().fg(t.warn),
            )),
        ],
        FocusPanel::Bloat => vec![
            heading("WHAT THIS SHOWS"),
            Line::from(format!(
                "{} evidence-backed findings in the quick scan.",
                app.bloat.findings().len()
            )),
            Line::from("Only deterministic generated artifacts can be removable."),
            Line::from(""),
            heading("ACTIONS"),
            action("6", "open findings and evidence"),
            action("g", "rescan from the Bloat screen"),
        ],
        FocusPanel::Sources => vec![
            heading("WHAT THIS SHOWS"),
            Line::from("Responding endpoints and installed runtime inventory."),
            Line::from("Loaded and installed states are intentionally separate."),
            Line::from(""),
            heading("ACTIONS"),
            action("m", "open model desk"),
            action("c", "configure a detected agent"),
        ],
    };
    lines.push(Line::from(""));
    lines.push(action("Esc", "return to the panel grid"));
    lines
}

fn evidence_heading(label: &str, t: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        label.to_string(),
        Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
    ))
}

fn evidence_fact(label: &str, value: impl Into<String>, t: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<13}"), Style::default().fg(t.dim)),
        Span::styled(value.into(), Style::default().fg(t.fg)),
    ])
}

const fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Queued => "queued",
        Stage::Prefill => "prefill",
        Stage::Decode => "decode",
        Stage::Done => "complete",
        Stage::Failed => "failed",
    }
}

fn prefix_reuse_summary(app: &App) -> String {
    match (app.metrics.prefix_hits, app.metrics.prefix_queries) {
        (Some(hits), Some(queries)) if queries > 0 => format!(
            "{:.1}% | {hits}/{queries} hits | {} partial | {} reused tok",
            hits as f64 / queries as f64 * 100.0,
            app.metrics.prefix_partial_hits.unwrap_or(0),
            app.metrics.prefix_reused_tokens.unwrap_or(0)
        ),
        _ => "not reported by this runtime".into(),
    }
}

fn kv_residency_summary(app: &App) -> String {
    let resident = app
        .metrics
        .kv_cache_resident_tokens
        .map(|value| format!("{value} resident tok"))
        .unwrap_or_else(|| "residency not reported".into());
    let evictions = app
        .metrics
        .kv_cache_evictions
        .map(|value| format!("{value} evictions"))
        .unwrap_or_else(|| "evictions not reported".into());
    format!("{resident} | {evictions}")
}

fn panel_evidence_lines(app: &App, panel: FocusPanel, width: u16) -> Vec<Line<'static>> {
    let t = &app.theme;
    let graph_width = width.saturating_sub(22).max(8) as usize;
    match panel {
        FocusPanel::HomeModel => {
            let served = app.served.iter().find(|server| server.port == app.port);
            let mut lines = vec![
                evidence_heading("RUNTIME IDENTITY", t),
                evidence_fact("state", if app.online { "responding" } else { "idle" }, t),
                evidence_fact("runtime", app.engine.clone(), t),
                evidence_fact("model", public_model_id(&app.model), t),
                evidence_fact(
                    "endpoint",
                    if app.online {
                        format!("127.0.0.1:{} | {:.0} ms probe", app.port, app.ping_ms)
                    } else {
                        "none responding".into()
                    },
                    t,
                ),
                evidence_fact(
                    "mode",
                    served
                        .and_then(|server| server.mode.clone())
                        .unwrap_or_else(|| "not reported".into()),
                    t,
                ),
                evidence_fact(
                    "drafter",
                    served
                        .and_then(|server| server.drafter.clone())
                        .unwrap_or_else(|| "none reported".into()),
                    t,
                ),
                evidence_fact(
                    "model detail",
                    format!(
                        "{} params | {} precision",
                        app.real_params.as_deref().unwrap_or("unknown"),
                        app.real_quant.as_deref().unwrap_or("unknown")
                    ),
                    t,
                ),
                Line::from(""),
                evidence_heading("CURRENT REQUEST", t),
            ];
            if let Some(request) = &app.current {
                lines.extend([
                    evidence_fact("stage", stage_name(request.stage), t),
                    evidence_fact("request", request.id.clone(), t),
                    evidence_fact(
                        "tokens",
                        format!(
                            "{} prompt | {} prefilled | {} decoded",
                            request.prompt_tokens, request.prefill_done, request.decoded
                        ),
                        t,
                    ),
                    evidence_fact(
                        "rates",
                        format!(
                            "{:.0} prefill | {:.1} decode tok/s",
                            request.prefill_rate, request.decode_rate
                        ),
                        t,
                    ),
                ]);
            } else {
                lines.push(evidence_fact("request", "none in flight", t));
            }
            lines
        }
        FocusPanel::HomeCapacity => {
            let storage = app.device.storage();
            let ram_used = app.rss_gb + app.sys_used_gb;
            let storage_used = (storage.total_gib - storage.available_gib).max(0.0);
            vec![
                evidence_heading("DEVICE CAPACITY", t),
                evidence_fact("chip", app.chip.clone(), t),
                evidence_fact(
                    if platform::has_unified_memory() {
                        "unified RAM"
                    } else {
                        "system RAM"
                    },
                    format!("{:.1} GiB", app.total_mem_gb),
                    t,
                ),
                evidence_fact("used RAM", format!("{ram_used:.1} GiB"), t),
                evidence_fact("model RSS", format!("{:.1} GiB", app.rss_gb), t),
                evidence_fact("OS and apps", format!("{:.1} GiB", app.sys_used_gb), t),
                evidence_fact("available", format!("{:.1} GiB", app.headroom_gb), t),
                evidence_fact("swap", format!("{:.0} MiB", app.swap_mb), t),
                evidence_fact("host CPU", format!("{:.0}%", app.host_cpu_pct), t),
                Line::from(""),
                evidence_heading("MODEL FILESYSTEM", t),
                evidence_fact("used", format!("{storage_used:.1} GiB"), t),
                evidence_fact("available", format!("{:.1} GiB", storage.available_gib), t),
                evidence_fact("total", format!("{:.1} GiB", storage.total_gib), t),
                evidence_fact("download rule", "reported size plus 1 GiB reserve", t),
            ]
        }
        FocusPanel::HomeSources => {
            let checked = app
                .huggingface
                .entries()
                .iter()
                .filter(|entry| entry.manifest().is_some())
                .count();
            let downloaded = app
                .huggingface
                .entries()
                .iter()
                .filter(|entry| entry.installed())
                .count();
            let mut lines = vec![
                evidence_heading("RESPONDING ENDPOINTS", t),
                evidence_fact("count", app.served.len().to_string(), t),
            ];
            for server in &app.served {
                lines.push(evidence_fact(
                    &server.runtime,
                    format!(
                        "{} | {} | {}",
                        server.endpoint_label(),
                        server.state,
                        public_model_id(&server.model)
                    ),
                    t,
                ));
            }
            if app.served.is_empty() {
                lines.push(evidence_fact("status", "none responding", t));
            }
            lines.extend([
                Line::from(""),
                evidence_heading("LOCAL INVENTORY", t),
                evidence_fact("load targets", app.server.available.len().to_string(), t),
                evidence_fact("runtime rows", app.model_sources.len().to_string(), t),
                evidence_fact("HF checked", checked.to_string(), t),
                evidence_fact("HF installed", downloaded.to_string(), t),
                evidence_fact(
                    "local.ai",
                    if app
                        .local_ai
                        .reading_for(&app.chip, app.total_mem_gb)
                        .is_some()
                    {
                        "matching cached evidence"
                    } else {
                        "no matching cached evidence"
                    },
                    t,
                ),
            ]);
            for source in app.model_sources.iter().take(6) {
                lines.push(evidence_fact(
                    &source.runtime,
                    format!(
                        "{} | {} | {} | {}",
                        source.state, source.endpoint, source.label, source.detail
                    ),
                    t,
                ));
            }
            lines
        }
        FocusPanel::HomeNext => {
            let cue = monitoring::primary_cue(app);
            let mut lines = vec![
                evidence_heading("CURRENT CUE", t),
                evidence_fact("severity", cue.severity, t),
                evidence_fact("state", cue.title, t),
                evidence_fact("meaning", cue.detail, t),
                evidence_fact("evidence", cue.evidence, t),
                evidence_fact(
                    "action",
                    format!("{} | key {}", cue.action_label, cue.action_key),
                    t,
                ),
                Line::from(""),
                evidence_heading("LOCAL MODEL PATH", t),
                evidence_fact(
                    "1 discover",
                    "inspect hardware, runtimes, and local models",
                    t,
                ),
                evidence_fact("2 choose", "select a model that fits this machine", t),
                evidence_fact(
                    "3 run",
                    if app.online {
                        "model endpoint ready"
                    } else {
                        "waiting for a model"
                    },
                    t,
                ),
                evidence_fact("4 connect", "prepare a detected local client or agent", t),
                evidence_fact(
                    "5 understand",
                    "explain latency, queue, cache, and pressure",
                    t,
                ),
                Line::from(""),
                evidence_heading("DETECTED AGENTS", t),
                evidence_fact("count", app.agents.detected().len().to_string(), t),
                evidence_fact("direct", app.agents.direct_count().to_string(), t),
            ];
            for agent in app.agents.detected() {
                lines.push(evidence_fact(
                    agent.display_name,
                    format!(
                        "{} | {}",
                        if agent.direct {
                            "direct"
                        } else {
                            "proxy required"
                        },
                        agent.evidence
                    ),
                    t,
                ));
            }
            lines
        }
        FocusPanel::Performance => {
            let mut lines = vec![
                evidence_heading("MEASUREMENT CUSTODY", t),
                evidence_fact(
                    "priority",
                    "live request > latest round > runtime aggregate > estimate",
                    t,
                ),
                evidence_fact(
                    "runtime build",
                    app.metrics
                        .runtime_version
                        .as_deref()
                        .unwrap_or("not reported"),
                    t,
                ),
                evidence_fact(
                    "decode",
                    format!("{:.1} tok/s", app.real_tg.unwrap_or(0.0)),
                    t,
                ),
                evidence_fact(
                    "prefill",
                    format!("{:.0} tok/s", app.real_pp.unwrap_or(0.0)),
                    t,
                ),
                evidence_fact("engine CPU", format!("{:.0}%", app.cpu_pct), t),
            ];
            lines.push(Line::from(""));
            lines.push(evidence_heading("BENCHMARK DISTRIBUTION", t));
            if app.bench.results.len() >= 3 {
                lines.extend([
                    evidence_fact(
                        "decode",
                        format!(
                            "p50 {:.1} | p95 {:.1} tok/s",
                            percentile(app.bench.results.iter().map(|run| run.tg), 0.50)
                                .unwrap_or(0.0),
                            percentile(app.bench.results.iter().map(|run| run.tg), 0.95)
                                .unwrap_or(0.0)
                        ),
                        t,
                    ),
                    evidence_fact(
                        "TTFT",
                        format!(
                            "p50 {:.0} | p95 {:.0} ms",
                            percentile(app.bench.results.iter().map(|run| run.ttft_ms), 0.50)
                                .unwrap_or(0.0),
                            percentile(app.bench.results.iter().map(|run| run.ttft_ms), 0.95)
                                .unwrap_or(0.0)
                        ),
                        t,
                    ),
                    evidence_fact(
                        "TPOT",
                        format!(
                            "p50 {:.1} | p95 {:.1} ms/token",
                            percentile(app.bench.results.iter().map(|run| run.tpot_ms), 0.50)
                                .unwrap_or(0.0),
                            percentile(app.bench.results.iter().map(|run| run.tpot_ms), 0.95)
                                .unwrap_or(0.0)
                        ),
                        t,
                    ),
                ]);
            } else {
                lines.push(evidence_fact(
                    "status",
                    format!(
                        "{} runs; 3 required for percentiles",
                        app.bench.results.len()
                    ),
                    t,
                ));
            }
            if !app.bench.concurrency_results.is_empty() {
                lines.push(Line::from(""));
                lines.push(evidence_heading("CONCURRENCY SWEEP", t));
                for point in &app.bench.concurrency_results {
                    lines.push(evidence_fact(
                        &format!("c{}", point.concurrency),
                        format!(
                            "{:.1} system tok/s | {:.1} request tok/s | p95 {:.0} ms | TPOT {:.1} ms | {} errors",
                            point.system_tokens_per_second,
                            point.mean_request_tokens_per_second,
                            point.p95_latency_ms,
                            point.p95_tpot_ms,
                            point.errors
                        ),
                        t,
                    ));
                    lines.push(evidence_fact(
                        "pressure",
                        format!(
                            "wait {} | KV {} | RSS {:.1} GiB | swap {:.0} MiB",
                            point
                                .peak_waiting_requests
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "?".into()),
                            point
                                .peak_kv_cache_usage
                                .map(|value| format!("{:.0}%", value * 100.0))
                                .unwrap_or_else(|| "?".into()),
                            point.peak_server_rss_gib,
                            point.peak_swap_mib
                        ),
                        t,
                    ));
                    lines.push(evidence_fact(
                        "token count",
                        point.token_count_source.clone(),
                        t,
                    ));
                }
            }
            let budgets = report::budget_assessments(app);
            if !budgets.is_empty() {
                lines.push(Line::from(""));
                lines.push(evidence_heading("WORKLOAD BUDGETS", t));
                for budget in budgets {
                    lines.push(evidence_fact(
                        &budget.metric,
                        format!(
                            "{} | {} {:.1} {} | observed {} | {}",
                            budget.status,
                            if budget.relation == "at_most" {
                                "<="
                            } else {
                                ">="
                            },
                            budget.target,
                            budget.unit,
                            budget
                                .observed
                                .map(|value| format!("{value:.1}"))
                                .unwrap_or_else(|| "?".into()),
                            budget.evidence
                        ),
                        t,
                    ));
                }
            }
            lines.extend([
                Line::from(""),
                evidence_heading("SPECULATIVE DECODE", t),
                evidence_fact(
                    "mode",
                    app.metrics.mode.as_deref().unwrap_or("not reported"),
                    t,
                ),
                evidence_fact(
                    "acceptance",
                    app.metrics
                        .draft_acceptance
                        .map(|value| format!("{:.1}%", value * 100.0))
                        .unwrap_or_else(|| "not reported".into()),
                    t,
                ),
                evidence_fact(
                    "mean accepted",
                    app.metrics
                        .mean_accept_len
                        .map(|value| format!("{value:.2} tokens per round"))
                        .unwrap_or_else(|| "not reported".into()),
                    t,
                ),
                evidence_fact("rounds", app.metrics.rounds.unwrap_or(0).to_string(), t),
                evidence_fact(
                    "committed",
                    app.metrics.committed_tokens.unwrap_or(0).to_string(),
                    t,
                ),
                evidence_fact("prefix reuse", prefix_reuse_summary(app), t),
                evidence_fact(
                    "batching",
                    format!(
                        "max {} | {} batches | {} requests",
                        app.metrics.batch_max.unwrap_or(0),
                        app.metrics.batch_batches.unwrap_or(0),
                        app.metrics.batch_requests.unwrap_or(0)
                    ),
                    t,
                ),
                evidence_fact(
                    "scheduler",
                    format!(
                        "{} running | {} waiting | {} swapped",
                        app.metrics.requests_running.unwrap_or(0),
                        app.metrics.requests_waiting.unwrap_or(0),
                        app.metrics.requests_swapped.unwrap_or(0)
                    ),
                    t,
                ),
                evidence_fact(
                    "KV capacity",
                    app.metrics
                        .kv_cache_usage
                        .map(|usage| format!("{:.1}% runtime-reported", usage * 100.0))
                        .unwrap_or_else(|| "not reported by this runtime".into()),
                    t,
                ),
            ]);
            if let Some(round) = &app.latest_round {
                lines.extend([
                    Line::from(""),
                    evidence_heading("LATEST VERIFIED ROUND", t),
                    evidence_fact(
                        "tokens",
                        format!(
                            "{} drafted | {} accepted | {} committed",
                            round.drafted, round.accepted, round.committed
                        ),
                        t,
                    ),
                    evidence_fact("duration", format!("{:.1} ms", round.ms), t),
                    evidence_fact(
                        "source",
                        if round.source.is_empty() {
                            "target".into()
                        } else {
                            round.source.clone()
                        },
                        t,
                    ),
                ]);
            }
            lines
        }
        FocusPanel::Streams => {
            let signals = [
                ("decode", "tok/s", &app.tok_hist, t.ok, false),
                ("prefill", "tok/s", &app.prefill_hist, t.accent, false),
                ("TTFT", "ms", &app.ttft_hist, t.warn, false),
                ("KV use", "%", &app.kv_hist, t.kv, true),
                ("waiting", "requests", &app.queue_hist, t.err, false),
                ("acceptance", "%", &app.acceptance_hist, t.kv, true),
                ("engine CPU", "%", &app.load_hist, t.warn, true),
            ];
            let mut lines = vec![
                evidence_heading("TRACKED INFERENCE SIGNALS", t),
                evidence_fact("focus", app.cfg.observability.focus(), t),
                evidence_fact(
                    "custody",
                    format!(
                        "{} in-memory poll slots | session only",
                        app.cfg.observability.history_samples()
                    ),
                    t,
                ),
            ];
            for (label, unit, history, color, percent_scale) in signals {
                let (count, minimum, mean, maximum) = sample_stats(history);
                let scale = if percent_scale {
                    100.0
                } else {
                    maximum.max(1.0)
                };
                lines.push(evidence_fact(
                    label,
                    format!(
                        "{count} samples | {minimum:.1} min | {mean:.1} mean | {maximum:.1} max {unit}"
                    ),
                    t,
                ));
                lines.push(Line::from(Span::styled(
                    sparkline(
                        history,
                        graph_width,
                        scale,
                        &app.visualization.graph_renderer,
                    ),
                    Style::default().fg(color),
                )));
            }
            lines.extend([
                evidence_fact(
                    "scaling",
                    "rates use observed maxima; percentages use 0-100",
                    t,
                ),
                evidence_fact(
                    "provenance",
                    "request timings, runtime metrics, and local process samples stay distinct",
                    t,
                ),
            ]);
            lines
        }
        FocusPanel::Stages => {
            let mut lines = vec![
                evidence_heading("LATEST REQUEST", t),
                evidence_fact("retained", app.spans.len().to_string(), t),
            ];
            if let Some(request) = app.selected_request() {
                let elapsed = request.last_update.duration_since(request.started);
                lines.extend([
                    evidence_fact("id", request.id.clone(), t),
                    evidence_fact("stage", stage_name(request.stage), t),
                    evidence_fact("elapsed", fmt_dur(elapsed), t),
                    evidence_fact(
                        "scheduler now",
                        if app.metrics.requests_running.is_some()
                            || app.metrics.requests_waiting.is_some()
                        {
                            format!(
                                "{} running | {} waiting | {} swapped",
                                app.metrics.requests_running.unwrap_or(0),
                                app.metrics.requests_waiting.unwrap_or(0),
                                app.metrics.requests_swapped.unwrap_or(0)
                            )
                        } else {
                            "not reported by this runtime".into()
                        },
                        t,
                    ),
                    evidence_fact("prompt", format!("{} tokens", request.prompt_tokens), t),
                    evidence_fact("prefilled", format!("{} tokens", request.prefill_done), t),
                    evidence_fact("decoded", format!("{} tokens", request.decoded), t),
                    evidence_fact(
                        "TTFT",
                        request
                            .ttft()
                            .map(|value| format!("{:.0} ms", value.as_secs_f64() * 1000.0))
                            .unwrap_or_else(|| "not observed".into()),
                        t,
                    ),
                    evidence_fact(
                        "rates",
                        format!(
                            "{:.0} prefill | {:.1} decode tok/s",
                            request.prefill_rate, request.decode_rate
                        ),
                        t,
                    ),
                    evidence_fact("sampling", request.sampling_summary(), t),
                    evidence_fact(
                        "cached",
                        request
                            .cached_tokens
                            .map(|value| format!("{value} prompt tokens"))
                            .unwrap_or_else(|| "not reported".into()),
                        t,
                    ),
                    evidence_fact(
                        "KV capacity",
                        app.metrics
                            .kv_cache_usage
                            .map(|usage| format!("{:.1}% runtime-reported", usage * 100.0))
                            .unwrap_or_else(|| {
                                "not reported; token ceiling shown separately".into()
                            }),
                        t,
                    ),
                    evidence_fact(
                        "verification",
                        app.metrics
                            .draft_acceptance
                            .map(|value| format!("{:.1}% speculative acceptance", value * 100.0))
                            .unwrap_or_else(|| "target-only or not reported".into()),
                        t,
                    ),
                    evidence_fact("prefix reuse", prefix_reuse_summary(app), t),
                    evidence_fact("KV residency", kv_residency_summary(app), t),
                    evidence_fact(
                        "context ceiling",
                        format!(
                            "{} tokens | {}",
                            app.ceiling.effective_max(),
                            if app.ceiling.kv_rate_real {
                                "architecture-derived estimate"
                            } else {
                                "estimated"
                            }
                        ),
                        t,
                    ),
                ]);
            } else {
                lines.push(evidence_fact("status", "no request observed yet", t));
            }
            lines.push(Line::from(""));
            lines.push(evidence_heading("RECENT STAGES", t));
            for request in app.spans.iter().rev().take(8) {
                lines.push(evidence_fact(
                    &clip(&request.id, 10),
                    format!(
                        "{} | {} prompt -> {} decoded",
                        stage_name(request.stage),
                        request.prompt_tokens,
                        request.decoded
                    ),
                    t,
                ));
            }
            lines
        }
        FocusPanel::History => {
            let completed = app
                .spans
                .iter()
                .filter(|request| request.stage == Stage::Done)
                .count();
            let failed = app
                .spans
                .iter()
                .filter(|request| request.stage == Stage::Failed)
                .count();
            let mut lines = vec![
                evidence_heading("LOCAL REQUEST LEDGER", t),
                evidence_fact(
                    "retained",
                    format!(
                        "{} of {} session records",
                        app.spans.len(),
                        app.cfg.observability.request_retention()
                    ),
                    t,
                ),
                evidence_fact("completed", completed.to_string(), t),
                evidence_fact("failed", failed.to_string(), t),
                evidence_fact("privacy", "prompt and response bodies are omitted", t),
                Line::from(""),
                evidence_heading("RECENT REQUESTS", t),
            ];
            for request in app.spans.iter().rev() {
                lines.push(evidence_fact(
                    &clip(&request.id, 10),
                    format!(
                        "{} | {} in -> {} out | TTFT {} | {:.1} tok/s",
                        stage_name(request.stage),
                        request.prompt_tokens,
                        request.decoded,
                        request
                            .ttft()
                            .map(|value| format!("{:.0} ms", value.as_secs_f64() * 1000.0))
                            .unwrap_or_else(|| "-".into()),
                        request.decode_rate
                    ),
                    t,
                ));
            }
            if app.spans.is_empty() {
                lines.push(evidence_fact("status", "empty; run a local workload", t));
            }
            lines
        }
        FocusPanel::Memory => {
            let active = app.metrics.memory_active_bytes.unwrap_or(0) as f64 / BYTES_PER_GIB;
            let peak = app.metrics.memory_peak_bytes.unwrap_or(0) as f64 / BYTES_PER_GIB;
            let cache = app.metrics.memory_cache_bytes.unwrap_or(0) as f64 / BYTES_PER_GIB;
            let mut lines = vec![
                evidence_heading("HOST MEMORY ACCOUNTING", t),
                evidence_fact("platform", platform::os_name(), t),
                evidence_fact("memory kind", platform::memory_kind(), t),
                evidence_fact("total RAM", format!("{:.1} GiB", app.total_mem_gb), t),
                evidence_fact("server RSS", format!("{:.1} GiB", app.rss_gb), t),
                evidence_fact("OS and apps", format!("{:.1} GiB", app.sys_used_gb), t),
                evidence_fact("headroom", format!("{:.1} GiB", app.headroom_gb), t),
                evidence_fact("swap", format!("{:.0} MiB", app.swap_mb), t),
            ];
            if platform::has_unified_memory() {
                lines.extend([
                    evidence_fact(
                        "weights",
                        format!(
                            "{}{:.1} GiB inside unified allocation",
                            if app.real_vram_gb.is_some() { "" } else { "~" },
                            app.weights_gb
                        ),
                        t,
                    ),
                    evidence_fact(
                        "KV cache",
                        format!(
                            "{}{:.2} GiB | {} tokens",
                            if app.metrics.kv_cache_tokens.is_some() {
                                ""
                            } else {
                                "~"
                            },
                            app.kv_gb,
                            app.metrics.kv_cache_tokens.unwrap_or(0)
                        ),
                        t,
                    ),
                ]);
            } else {
                lines.push(evidence_fact(
                    "device model",
                    app.real_vram_gb
                        .map(|value| {
                            format!("{value:.2} GiB runtime-reported; separate from server RSS")
                        })
                        .unwrap_or_else(|| "not reported; not estimated inside RSS".into()),
                    t,
                ));
            }
            lines.extend([
                Line::from(""),
                evidence_heading("RUNTIME ALLOCATOR", t),
                evidence_fact(
                    "active",
                    if app.metrics.memory_active_bytes.is_some() {
                        format!("{active:.2} GiB")
                    } else {
                        "not reported".into()
                    },
                    t,
                ),
                evidence_fact(
                    "peak",
                    if app.metrics.memory_peak_bytes.is_some() {
                        format!("{peak:.2} GiB")
                    } else {
                        "not reported".into()
                    },
                    t,
                ),
                evidence_fact(
                    "cache",
                    if app.metrics.memory_cache_bytes.is_some() {
                        format!("{cache:.2} GiB")
                    } else {
                        "not reported".into()
                    },
                    t,
                ),
                evidence_fact(
                    "context",
                    format!(
                        "{} used | {} effective maximum | {}",
                        app.ceiling.current_tokens,
                        app.ceiling.effective_max(),
                        match app.ceiling.binding {
                            Binding::Model => "model-bound",
                            Binding::Memory => "memory-bound",
                            Binding::Unknown => "binding unknown",
                        }
                    ),
                    t,
                ),
            ]);
            lines
        }
        FocusPanel::Pressure => {
            let mut lines = vec![
                evidence_heading("SYSTEM CONDITIONS", t),
                evidence_fact("platform", platform::os_name(), t),
                evidence_fact(
                    "process list",
                    if app.interference.paused {
                        "paused"
                    } else {
                        "live"
                    },
                    t,
                ),
                evidence_fact("host CPU", format!("{:.0}%", app.host_cpu_pct), t),
                evidence_fact("headroom", format!("{:.1} GiB", app.headroom_gb), t),
                evidence_fact("swap", format!("{:.0} MiB", app.swap_mb), t),
                evidence_fact(
                    "power mode",
                    if app.interference.low_power {
                        "low power enabled"
                    } else {
                        "normal"
                    },
                    t,
                ),
                evidence_fact(
                    "CPU limit",
                    app.interference
                        .cpu_speed_limit
                        .map(|value| format!("{value}%"))
                        .unwrap_or_else(|| "not reported".into()),
                    t,
                ),
                Line::from(""),
                evidence_heading("OUTSIDE PROCESSES", t),
            ];
            for offender in &app.interference.offenders {
                lines.push(evidence_fact(
                    &format!("PID {}", offender.pid),
                    format!(
                        "{} | {:.1} GiB | {:.0}% CPU | {}",
                        offender.name, offender.mem_gb, offender.cpu, offender.hint
                    ),
                    t,
                ));
            }
            if app.interference.offenders.is_empty() {
                lines.push(evidence_fact("status", "no material contention", t));
            }
            for warning in &app.interference.warnings {
                lines.push(evidence_fact("warning", warning.clone(), t));
            }
            lines.push(evidence_fact(
                "guardrail",
                "Tokoro and terminal processes cannot be terminated",
                t,
            ));
            lines
        }
        FocusPanel::Bloat => {
            let findings = app.bloat.findings();
            let safe = findings
                .iter()
                .filter(|finding| finding.can_remove())
                .count();
            let reclaim = findings
                .iter()
                .map(|finding| finding.reclaim_bytes)
                .sum::<u64>();
            let mut lines = vec![
                evidence_heading("BOUNDED LOCAL SCAN", t),
                evidence_fact("scope", app.bloat.scan_summary(), t),
                evidence_fact(
                    "scan age",
                    app.bloat
                        .scan_age_seconds()
                        .map(|seconds| format!("{seconds} seconds"))
                        .unwrap_or_else(|| "not completed".into()),
                    t,
                ),
                evidence_fact("findings", findings.len().to_string(), t),
                evidence_fact("safe", safe.to_string(), t),
                evidence_fact("reclaimable", bloat::format_bytes(reclaim), t),
                evidence_fact(
                    "removal rule",
                    "deterministic generated artifacts only; human confirmation required",
                    t,
                ),
                Line::from(""),
                evidence_heading("FINDINGS", t),
            ];
            for finding in &findings {
                lines.push(evidence_fact(
                    finding.disposition.label(),
                    format!(
                        "{} | {} | {}",
                        finding.confidence.label(),
                        finding.title,
                        finding.evidence
                    ),
                    t,
                ));
            }
            if findings.is_empty() {
                lines.push(evidence_fact("status", "no threshold crossed", t));
            }
            lines
        }
        FocusPanel::Sources => {
            let mut lines = vec![
                evidence_heading("RESPONDING SERVERS", t),
                evidence_fact("platform", platform::os_name(), t),
                evidence_fact("count", app.served.len().to_string(), t),
            ];
            for server in &app.served {
                lines.extend([
                    evidence_fact(
                        &server.runtime,
                        format!(
                            "{} | {} | {}",
                            server.endpoint_label(),
                            server.state,
                            public_model_id(&server.model)
                        ),
                        t,
                    ),
                    evidence_fact(
                        "serve detail",
                        format!(
                            "mode {} | owner {} | drafter {}",
                            server.mode.as_deref().unwrap_or("not reported"),
                            server.owner.as_deref().unwrap_or("not reported"),
                            server.drafter.as_deref().unwrap_or("not reported")
                        ),
                        t,
                    ),
                ]);
            }
            if app.served.is_empty() {
                lines.push(evidence_fact("status", "no responding endpoint", t));
            }
            lines.extend([
                Line::from(""),
                evidence_heading("TELEMETRY CUSTODY", t),
                evidence_fact(
                    "sample age",
                    app.runtime_observed_at
                        .map(|observed| format!("{} seconds", observed.elapsed().as_secs()))
                        .unwrap_or_else(|| "no completed probe".into()),
                    t,
                ),
                evidence_fact(
                    "performance",
                    if app.real_tg.is_some() || app.real_pp.is_some() {
                        "runtime-reported"
                    } else {
                        "not reported"
                    },
                    t,
                ),
                evidence_fact(
                    "speculation",
                    if app.metrics.draft_acceptance.is_some() {
                        "runtime-reported"
                    } else {
                        "not reported"
                    },
                    t,
                ),
                evidence_fact(
                    "allocator",
                    if app.metrics.memory_active_bytes.is_some() {
                        "runtime-reported"
                    } else {
                        "not reported"
                    },
                    t,
                ),
                evidence_fact("inventory", "kept separate in Overview / Inventory", t),
            ]);
            lines
        }
    }
}

fn render_panel_evidence(f: &mut Frame, area: Rect, app: &App, panel: FocusPanel) {
    let block = panel_block("FULL EVIDENCE", &app.theme);
    f.render_widget(block.clone(), area);
    let inner = block.inner(area);
    f.render_widget(
        Paragraph::new(panel_evidence_lines(app, panel, inner.width)).wrap(Wrap { trim: true }),
        inner,
    );
}

fn expanded_summary_height(panel: FocusPanel, available: u16) -> u16 {
    let preferred = match panel {
        FocusPanel::HomeModel => 11,
        FocusPanel::HomeCapacity => 9,
        FocusPanel::HomeSources => 10,
        FocusPanel::HomeNext => 10,
        FocusPanel::Performance => 14,
        FocusPanel::Streams => 8,
        FocusPanel::Stages => 7,
        FocusPanel::History => 9,
        FocusPanel::Memory => 12,
        FocusPanel::Pressure => 11,
        FocusPanel::Bloat => 9,
        FocusPanel::Sources => 8,
    };
    preferred.min(available.saturating_sub(7)).max(5)
}

fn render_expanded_content(f: &mut Frame, area: Rect, app: &App, panel: FocusPanel) {
    if area.height >= 16 {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(expanded_summary_height(panel, area.height)),
                Constraint::Min(7),
            ])
            .split(area);
        render_panel_content(f, rows[0], app, panel);
        render_panel_evidence(f, rows[1], app, panel);
    } else {
        render_panel_evidence(f, area, app, panel);
    }
    render_panel_ring(f, area, app, panel, true);
}

fn panel_provenance(app: &App, panel: FocusPanel) -> String {
    match panel {
        FocusPanel::HomeModel | FocusPanel::HomeSources | FocusPanel::Sources => app
            .runtime_observed_at
            .map(|observed| format!("runtime probe | {}s old", observed.elapsed().as_secs()))
            .unwrap_or_else(|| "runtime probe | no completed sample".into()),
        FocusPanel::HomeCapacity | FocusPanel::Memory | FocusPanel::Pressure => {
            "local OS sample | live session".into()
        }
        FocusPanel::Performance | FocusPanel::Stages => {
            "live request, runtime report, then estimate".into()
        }
        FocusPanel::Streams => "in-memory session samples | not persisted".into(),
        FocusPanel::History => "local request ledger | bodies excluded".into(),
        FocusPanel::Bloat => "bounded local scan | evidence required".into(),
        FocusPanel::HomeNext => "derived from current local state".into(),
    }
}

fn render_panel_guide(f: &mut Frame, area: Rect, app: &App, panel: FocusPanel) {
    let focused = app.expanded_pane == ExpandedPane::Guide;
    let block = Block::default()
        .title(if focused {
            " DETAIL / ACTIONS [FOCUSED] "
        } else {
            " DETAIL / ACTIONS "
        })
        .title_bottom(
            Line::from(if focused {
                " 2/2 | j/k choose | Enter run "
            } else {
                " 2/2 | Tab to focus "
            })
            .style(Style::default().fg(if focused {
                app.theme.accent
            } else {
                app.theme.dim
            }))
            .right_aligned(),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused {
            app.theme.accent
        } else {
            app.theme.dim
        }));
    f.render_widget(block.clone(), area);
    let explanation_count = match panel {
        FocusPanel::HomeNext | FocusPanel::Pressure => 2,
        _ => 3,
    };
    let mut lines = panel_guide_lines(app, panel)
        .into_iter()
        .take(explanation_count)
        .collect::<Vec<_>>();
    lines.extend([
        Line::from(""),
        evidence_heading("PROVENANCE", &app.theme),
        Line::from(panel_provenance(app, panel)),
        Line::from(""),
        evidence_heading("ACTIONS", &app.theme),
    ]);
    let actions = app.panel_actions(panel);
    let selected = app.expanded_action_sel.min(actions.len().saturating_sub(1));
    for (index, action) in actions.iter().enumerate() {
        let active = focused && index == selected;
        lines.push(Line::from(vec![
            Span::styled(
                if active { "> " } else { "  " },
                Style::default().fg(if active {
                    app.theme.accent
                } else {
                    app.theme.dim
                }),
            ),
            Span::styled(
                format!("{:<6}", action.key),
                Style::default().fg(if action.enabled {
                    app.theme.accent
                } else {
                    app.theme.dim
                }),
            ),
            Span::styled(
                action.label,
                Style::default()
                    .fg(if active { app.theme.fg } else { app.theme.dim })
                    .add_modifier(if active {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            format!("        {}", action.detail),
            Style::default().fg(app.theme.dim),
        )));
    }
    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            "Esc returns to the unchanged grid.",
            Style::default().fg(app.theme.dim),
        )),
    ]);
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }),
        block.inner(area),
    );
}

fn render_expanded_panel(f: &mut Frame, area: Rect, app: &App, panel: FocusPanel) {
    if area.width >= 72 {
        let (content_width, action_width) = if area.width >= 110 {
            (68, 32)
        } else {
            (60, 40)
        };
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(content_width),
                Constraint::Percentage(action_width),
            ])
            .split(area);
        render_expanded_content(f, columns[0], app, panel);
        render_panel_guide(f, columns[1], app, panel);
    } else if app.expanded_pane == ExpandedPane::Guide {
        render_panel_guide(f, area, app, panel);
    } else {
        render_expanded_content(f, area, app, panel);
    }
}

fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(v[1])[1]
}

const fn workspace_popup(area: Rect) -> Rect {
    area
}

fn visible_start(selected: usize, count: usize, visible: usize) -> usize {
    if visible == 0 || count <= visible {
        0
    } else {
        selected
            .saturating_sub(visible / 2)
            .min(count.saturating_sub(visible))
    }
}

pub(crate) fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    if let Some(bg) = app.theme.bg {
        f.render_widget(Block::default().style(Style::default().bg(bg)), area);
    }
    if area.width < 24 || area.height < 3 {
        f.render_widget(
            Paragraph::new("tokoro: terminal too small (need 24x3)")
                .style(Style::default().fg(app.theme.warn)),
            area,
        );
        return;
    }

    let header_height = if area.height >= 10 { 2 } else { 1 };
    let status_height = if app.status_msg.is_some() && area.height >= 8 {
        1
    } else {
        0
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(1),
            Constraint::Length(status_height),
        ])
        .split(area);
    render_header(f, chunks[0], app);
    let body = chunks[1];
    if let Some(panel) = app
        .expanded_panel
        .filter(|panel| panel.screen() == app.screen)
    {
        render_expanded_panel(f, body, app, panel);
    } else {
        match app.screen {
            Screen::Home => render_home(f, body, app),
            Screen::Measure => render_measure(f, body, app),
            Screen::System => render_system(f, body, app),
            Screen::Learn => render_learn_screen(f, body, app),
            Screen::Customize => render_customize(f, body, app),
            Screen::Bloat => render_bloat_screen(f, body, app),
        }
    }
    if let Some((msg, _)) = &app.status_msg {
        if status_height > 0 {
            f.render_widget(
                Paragraph::new(format!("! {}", msg)).style(Style::default().fg(app.theme.warn)),
                chunks[2],
            );
        }
    }

    match app.popup {
        Popup::Command => render_command_popup(f, area, app),
        Popup::Models => render_models_popup(f, area, app, &app.theme),
        Popup::Connect => render_connect_popup(f, area, app, &app.theme),
        Popup::ConnectModels => render_connect_models_popup(f, area, app, &app.theme),
        Popup::Benchmarks => render_benchmarks_popup(f, area, app, &app.theme),
        Popup::Panels => render_panels_popup(f, area, app),
        Popup::Themes => render_themes_popup(f, area, app),
        Popup::Publish => render_publish_popup(f, area, app, &app.theme),
        Popup::Onboarding => render_onboarding_popup(f, area, app),
        Popup::None => {}
    }
}

fn render_header(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let state = if app.online {
        format!("LIVE :{}", app.port)
    } else {
        "IDLE".into()
    };
    let identity = if app.online {
        public_model_id(&app.model)
    } else {
        screen_name(app.screen).to_string()
    };
    let identity = clip(&identity, r.width.saturating_sub(28) as usize);
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "tokoro",
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" / ", Style::default().fg(t.dim)),
        Span::styled(identity, Style::default().fg(t.fg)),
        Span::styled("  ", Style::default().fg(t.dim)),
        Span::styled(
            state,
            Style::default()
                .fg(if app.online { t.ok } else { t.dim })
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    if r.height > 1 {
        let context = if let Some(panel) = app.expanded_panel {
            format!(
                "{} / {} | {} | Tab next focus | Shift-Tab previous | Esc back",
                screen_name(app.screen),
                focus_panel_title(panel, app).to_lowercase(),
                match app.expanded_pane {
                    ExpandedPane::Content => "evidence 1/2",
                    ExpandedPane::Guide => "details/actions 2/2",
                }
            )
        } else if let Some((position, total)) = app.selected_panel_position() {
            format!(
                "Panel {position}/{total} | Tab next | Shift-Tab previous | Enter open | / commands"
            )
        } else {
            match app.screen {
                Screen::Learn => "j/k lessons  m models  b benchmark  / commands".into(),
                Screen::Customize => "j/k setting  Enter change  P panels  / commands".into(),
                Screen::Bloat => "j/k finding  g scan  D deep scan  / commands".into(),
                _ => "/ commands  1-6 screens".into(),
            }
        };
        lines.push(Line::from(Span::styled(
            clip(&context, r.width as usize),
            Style::default().fg(t.dim),
        )));
    }
    f.render_widget(Paragraph::new(lines), r);
}

fn clip(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= width {
        value.into()
    } else if width <= 1 {
        "~".into()
    } else {
        format!("{}~", chars[..width - 1].iter().collect::<String>())
    }
}

fn render_home(f: &mut Frame, r: Rect, app: &App) {
    if r.height < 7 {
        render_compact_home(f, r, app);
        return;
    }

    if app.visualization.is_stacked() && r.height >= 28 {
        render_vertical_profile(f, r, app, "overview panels unavailable");
        return;
    }

    if r.width < 76 || app.visualization.is_focused() || app.visualization.is_stacked() {
        let panel = app.selected_panel().unwrap_or(FocusPanel::HomeModel);
        render_panel_content(f, r, app, panel);
        render_panel_ring(f, r, app, panel, false);
        return;
    }

    let dense = app.visualization.layout == "dense";
    let column_weights = if dense {
        [Constraint::Fill(1), Constraint::Fill(1)]
    } else {
        match app.cfg.layout.density.as_str() {
            "expanded" => [Constraint::Fill(7), Constraint::Fill(5)],
            _ => [Constraint::Fill(3), Constraint::Fill(2)],
        }
    };
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(column_weights)
        .split(r);
    let first_height = if dense {
        (r.height / 2).max(7)
    } else if r.height >= 28 {
        12
    } else {
        (r.height / 2).max(7)
    };
    let second_height = if dense {
        (r.height / 2).max(7)
    } else if r.height >= 28 {
        11
    } else {
        (r.height / 2).max(7)
    };
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(first_height.min(r.height.saturating_sub(7))),
            Constraint::Min(7),
        ])
        .split(columns[0]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(second_height.min(r.height.saturating_sub(7))),
            Constraint::Min(7),
        ])
        .split(columns[1]);
    let panels = app.visible_panels();
    for (area, panel) in [left[0], right[0], left[1], right[1]]
        .into_iter()
        .zip(panels)
    {
        render_panel_content(f, area, app, panel);
        render_panel_ring(f, area, app, panel, false);
    }
}

fn render_compact_home(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let storage = app.device.storage();
    let ram_used = app.rss_gb + app.sys_used_gb;
    let state = if app.online {
        format!(
            "LIVE :{}  {}",
            app.port,
            clip(&public_model_id(&app.model), 30)
        )
    } else {
        "NO MODEL LOADED".into()
    };
    let mut lines = vec![Line::from(Span::styled(
        state,
        Style::default()
            .fg(if app.online { t.ok } else { t.fg })
            .add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(vec![
        Span::styled("DEVICE  ", Style::default().fg(t.dim)),
        Span::styled(
            clip(&app.chip, r.width.saturating_sub(24) as usize),
            Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "  /  {:.0} GiB {} RAM",
                app.total_mem_gb,
                platform::memory_kind()
            ),
            Style::default().fg(t.dim),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("MODELS  ", Style::default().fg(t.dim)),
        Span::styled(
            format!("{:.1} GiB disk free", storage.available_gib),
            Style::default().fg(t.weights).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  /  RAM ", Style::default().fg(t.dim)),
        Span::styled(
            format!("{:.1}/{:.0} GiB used", ram_used, app.total_mem_gb),
            Style::default().fg(t.fg),
        ),
    ]));
    if app.online {
        lines.push(Line::from(format!(
            "decode {:.1} tok/s | {:.1} GiB RAM available | context {}k/{}k",
            app.real_tg.unwrap_or(0.0),
            app.headroom_gb,
            app.ceiling.current_tokens / 1000,
            app.ceiling.effective_max() / 1000
        )));
        lines.push(Line::from("b benchmark | c configure agent | m models"));
    } else {
        let sourced = if app
            .local_ai
            .reading_for(&app.chip, app.total_mem_gb)
            .is_some()
        {
            "public local.ai cached"
        } else {
            "local.ai optional"
        };
        lines.push(Line::from(vec![
            Span::styled("AGENTS  ", Style::default().fg(t.dim)),
            Span::styled(
                format!("{} detected", app.agents.detected().len()),
                Style::default().fg(t.accent),
            ),
            Span::styled(format!("  /  {sourced}"), Style::default().fg(t.dim)),
        ]));
        lines.push(Line::from(Span::styled(
            "m local  h Hugging Face  l sourced comparison",
            Style::default().fg(t.accent),
        )));
    }
    f.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(t.fg))
            .wrap(Wrap { trim: true }),
        r,
    );
}

fn render_home_sources(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let b = panel_block("INVENTORY", t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let installed = app
        .model_sources
        .iter()
        .filter(|source| source.state == "installed")
        .count();
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!(
                "{} responding  ",
                app.served
                    .iter()
                    .filter(|server| server.state == "loaded")
                    .count()
            ),
            Style::default().fg(if app.online { t.ok } else { t.dim }),
        ),
        Span::styled(
            format!(
                "{} installed  {} local target{}",
                installed,
                app.server.available.len(),
                if app.server.available.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            Style::default().fg(t.dim),
        ),
    ])];
    for server in app.served.iter().take(3) {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}  ", server.endpoint_label()),
                Style::default().fg(t.ok),
            ),
            Span::styled(
                format!("{:<12}", clip(&server.runtime, 12)),
                Style::default().fg(t.fg),
            ),
            Span::styled(
                clip(
                    &public_model_id(&server.model),
                    inner.width.saturating_sub(21) as usize,
                ),
                Style::default().fg(t.dim),
            ),
        ]));
    }
    if app.served.is_empty() {
        lines.push(Line::from(Span::styled(
            "No endpoint responds. Installed models remain inactive.",
            Style::default().fg(t.dim),
        )));
    }

    lines.push(Line::from(Span::styled(
        "LOCAL TARGETS",
        Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
    )));
    let footer_rows = 2usize;
    let target_rows = (inner.height as usize)
        .saturating_sub(lines.len() + footer_rows)
        .max(1);
    if app.server.catalog_loading() {
        lines.push(Line::from(Span::styled(
            "  runtime catalog scan in progress",
            Style::default().fg(t.warn),
        )));
    } else if app.server.available.is_empty() {
        lines.push(Line::from(Span::styled(
            "  none found in the configured models directory",
            Style::default().fg(t.dim),
        )));
    } else {
        for choice in app.server.available.iter().take(target_rows) {
            lines.push(Line::from(vec![
                Span::styled(
                    if choice.can_start { "+ " } else { "~ " },
                    Style::default().fg(if choice.can_start { t.ok } else { t.dim }),
                ),
                Span::styled(
                    clip(&choice.label, inner.width.saturating_sub(4) as usize),
                    Style::default().fg(t.fg),
                ),
            ]));
        }
    }

    let entries = app.huggingface.entries();
    let checked = entries
        .iter()
        .filter(|entry| entry.manifest().is_some())
        .count();
    let downloaded = entries.iter().filter(|entry| entry.installed()).count();
    lines.push(Line::from(vec![
        Span::styled("HF  ", Style::default().fg(t.dim)),
        Span::styled(
            format!("{checked} checked | {downloaded} downloaded"),
            Style::default().fg(if downloaded > 0 { t.ok } else { t.fg }),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("SOURCE  ", Style::default().fg(t.dim)),
        Span::styled(
            if app
                .local_ai
                .reading_for(&app.chip, app.total_mem_gb)
                .is_some()
            {
                "matching public comparison cached"
            } else {
                "no matching public comparison cached"
            },
            Style::default().fg(t.fg),
        ),
    ]));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_home_next(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let b = panel_block("NEXT", t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let cue = monitoring::primary_cue(app);
    let mut lines = vec![
        Line::from(Span::styled(
            format!("CURRENT CUE  {}", cue.severity.to_ascii_uppercase()),
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            cue.title,
            Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                format!("{}  ", cue.action_key),
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(cue.action_label, Style::default().fg(t.fg)),
        ]),
        Line::from(Span::styled(cue.detail, Style::default().fg(t.fg))),
    ];
    if inner.height >= 9 {
        lines.extend([
            Line::from(""),
            Line::from(Span::styled(
                "LOCAL MODEL PATH",
                Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
            )),
            workflow_line("discover", true, t),
            workflow_line("choose", app.online || !app.server.available.is_empty(), t),
            workflow_line("run", app.online, t),
            workflow_line(
                "connect",
                app.online && !app.agents.detected().is_empty(),
                t,
            ),
            workflow_line(
                "understand",
                app.online && (app.real_tg.is_some() || !app.bench.results.is_empty()),
                t,
            ),
        ]);
    }
    if inner.height as usize > lines.len() + 2 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "AGENTS  {} detected | {} direct",
                app.agents.detected().len(),
                app.agents.direct_count()
            ),
            Style::default().fg(t.dim),
        )));
        for agent in app
            .agents
            .detected()
            .iter()
            .take((inner.height as usize).saturating_sub(lines.len()))
        {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default().fg(t.dim)),
                Span::styled(agent.display_name, Style::default().fg(t.fg)),
                Span::styled(
                    if agent.direct { "  direct" } else { "  proxy" },
                    Style::default().fg(t.dim),
                ),
            ]));
        }
    }
    if inner.height as usize >= lines.len() + 4 {
        lines.extend([
            Line::from(""),
            Line::from(Span::styled(
                "GUARDRAILS",
                Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
            )),
            Line::from("downloads, serving, and publishing require explicit actions"),
            Line::from("local scans stay bounded; agents cannot delete findings"),
        ]);
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn workflow_line(label: &str, complete: bool, t: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            if complete { "[x] " } else { "[ ] " },
            Style::default().fg(if complete { t.ok } else { t.dim }),
        ),
        Span::styled(label.to_string(), Style::default().fg(t.fg)),
    ])
}

fn render_focus(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let title = if app.online { "RUNNING" } else { "MODEL" };
    let b = panel_block(title, t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let mut lines = Vec::new();
    if app.online {
        let served = app.served.iter().find(|server| server.port == app.port);
        let mode = served
            .and_then(|server| server.mode.as_deref())
            .unwrap_or("unknown mode");
        let drafter = served
            .and_then(|server| server.drafter.as_deref())
            .unwrap_or("none reported");
        lines.push(Line::from(Span::styled(
            format!("{}  :{}  {}", app.engine, app.port, mode),
            Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::styled("model  ", Style::default().fg(t.dim)),
            Span::styled(
                clip(
                    &public_model_id(&app.model),
                    inner.width.saturating_sub(8) as usize,
                ),
                Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            app.runtime_observed_at
                .map(|observed| {
                    format!(
                        "probe  {}s old | localhost | {:.0} ms",
                        observed.elapsed().as_secs(),
                        app.ping_ms
                    )
                })
                .unwrap_or_else(|| "probe  age unknown".into()),
            Style::default().fg(t.dim),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "draft  {}",
                clip(drafter, inner.width.saturating_sub(7) as usize)
            ),
            Style::default().fg(t.kv),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "speed  {:.1} tok/s decode  |  {:.0} tok/s prefill  |  TTFT {}",
                app.real_tg.unwrap_or(0.0),
                app.real_pp.unwrap_or(0.0),
                app.current
                    .as_ref()
                    .and_then(|request| request.ttft())
                    .map(|d| format!("{:.0} ms", d.as_secs_f64() * 1000.0))
                    .unwrap_or_else(|| "-".into())
            ),
            Style::default().fg(t.fg),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "memory {:.1} GiB headroom  |  context {}k / {}k  |  {}",
                app.headroom_gb,
                app.ceiling.current_tokens / 1000,
                app.ceiling.effective_max() / 1000,
                match app.ceiling.binding {
                    Binding::Model => "model ceiling",
                    Binding::Memory => "memory ceiling",
                    Binding::Unknown => "ceiling unknown",
                }
            ),
            Style::default().fg(t.dim),
        )));
        if let Some(request) = &app.current {
            lines.push(Line::from(Span::styled(
                format!(
                    "request {} | {} tok out{}",
                    match request.stage {
                        Stage::Prefill => "prefill",
                        Stage::Decode => "decode",
                        Stage::Done => "complete",
                        Stage::Failed => "failed",
                        Stage::Queued => "queued",
                    },
                    request.decoded,
                    request
                        .prefill_eta()
                        .map(|eta| format!(" | ETA {}", fmt_dur(eta)))
                        .unwrap_or_default()
                ),
                Style::default().fg(t.warn),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "ready",
                Style::default().fg(t.dim),
            )));
        }
        if inner.height >= 10 {
            let context_fraction =
                app.ceiling.current_tokens as f64 / app.ceiling.effective_max().max(1) as f64;
            let width = inner.width.saturating_sub(22).clamp(8, 36) as usize;
            lines.push(Line::from(vec![
                Span::styled("context  ", Style::default().fg(t.dim)),
                Span::styled(bar(context_fraction, width), Style::default().fg(t.kv)),
                Span::styled(
                    format!("  {:.0}%", context_fraction * 100.0),
                    Style::default().fg(t.fg),
                ),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "NO MODEL LOADED",
            Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "{} | {:.0} GiB {} RAM",
                app.chip,
                app.total_mem_gb,
                platform::memory_kind()
            ),
            Style::default().fg(t.dim),
        )));
        let target_count = app.server.available.len();
        lines.push(Line::from(Span::styled(
            format!(
                "{} load target{} found",
                target_count,
                if target_count == 1 { "" } else { "s" }
            ),
            Style::default().fg(t.fg),
        )));
        lines.push(Line::from(Span::styled(
            "m  choose a local model",
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "h  check and download pinned Hugging Face starters",
            Style::default().fg(t.fg),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "agents  {} detected; connect after loading a model",
                app.agents.detected().len()
            ),
            Style::default().fg(t.dim),
        )));
        if inner.height >= 9 {
            lines.push(Line::from(Span::styled(
                format!(
                    "host  {}{} | managed serving {}",
                    platform::os_name(),
                    if platform::is_omarchy() {
                        " / Omarchy"
                    } else {
                        ""
                    },
                    if app.server.managed_available {
                        "ready"
                    } else {
                        "not configured"
                    }
                ),
                Style::default().fg(t.dim),
            )));
        }
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_home_signals(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let b = panel_block("CAPACITY", t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let ram_used = app.rss_gb + app.sys_used_gb;
    let ram_fraction = ram_used / app.total_mem_gb.max(1.0);
    let storage = app.device.storage();
    let storage_used = (storage.total_gib - storage.available_gib).max(0.0);
    let storage_fraction = storage_used / storage.total_gib.max(1.0);
    let meter_width = inner.width.saturating_sub(20).clamp(10, 36) as usize;
    let mut lines = vec![
        Line::from(vec![
            Span::styled("RAM     ", Style::default().fg(t.dim)),
            Span::styled(
                format!("{:.1} / {:.0} GiB used", ram_used, app.total_mem_gb),
                Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{}  ", bar(ram_fraction, meter_width)),
                Style::default().fg(if app.headroom_gb >= 16.0 {
                    t.ok
                } else {
                    t.warn
                }),
            ),
            Span::styled(
                format!("{:.1} GiB available", app.headroom_gb),
                Style::default().fg(t.dim),
            ),
        ]),
        Line::from(vec![
            Span::styled("MODELS  ", Style::default().fg(t.dim)),
            Span::styled(
                format!("{:.1} GiB disk free", storage.available_gib),
                Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{}  ", bar(storage_fraction, meter_width)),
                Style::default().fg(t.weights),
            ),
            Span::styled(
                format!("{:.0} GiB total", storage.total_gib),
                Style::default().fg(t.dim),
            ),
        ]),
    ];
    if !platform::has_unified_memory() {
        lines.push(Line::from(vec![
            Span::styled("DEVICE  ", Style::default().fg(t.dim)),
            Span::styled(
                app.real_vram_gb
                    .map(|value| format!("{value:.1} GiB model memory reported"))
                    .unwrap_or_else(|| "model memory not reported".into()),
                Style::default().fg(if app.real_vram_gb.is_some() {
                    t.weights
                } else {
                    t.dim
                }),
            ),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("AGENTS  ", Style::default().fg(t.dim)),
        Span::styled(
            format!(
                "{} found | {} direct",
                app.agents.detected().len(),
                app.agents.direct_count()
            ),
            Style::default().fg(t.accent),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        format!(
            "swap {:.0} MiB | host CPU {:.0}%",
            app.swap_mb, app.host_cpu_pct
        ),
        Style::default().fg(if app.swap_mb > 500.0 { t.warn } else { t.dim }),
    )));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_measure(f: &mut Frame, r: Rect, app: &App) {
    if r.height < 7 {
        render_compact_measure(f, r, app);
    } else if app.visualization.is_stacked() && r.height >= 28 {
        render_vertical_profile(f, r, app, "measure panels hidden in setup");
    } else if r.width >= 76
        && r.height >= 12
        && !app.visualization.is_focused()
        && !app.visualization.is_stacked()
    {
        render_measure_grid(f, r, app);
    } else if let Some(panel) = app.selected_panel() {
        render_panel_content(f, r, app, panel);
        render_panel_ring(f, r, app, panel, false);
    } else {
        render_hidden_notice(f, r, app, "measure panels hidden in setup");
    }
}

fn render_panel_stack(
    f: &mut Frame,
    area: Rect,
    app: &App,
    panels: &[FocusPanel],
    constraints: [Constraint; 2],
    empty_message: &str,
) {
    match panels {
        [] => render_hidden_notice(f, area, app, empty_message),
        [panel] => {
            render_panel_content(f, area, app, *panel);
            render_panel_ring(f, area, app, *panel, false);
        }
        [first, second, ..] => {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(area);
            for (row, panel) in rows.iter().zip([*first, *second]) {
                render_panel_content(f, *row, app, panel);
                render_panel_ring(f, *row, app, panel, false);
            }
        }
    }
}

fn render_vertical_profile(f: &mut Frame, area: Rect, app: &App, empty_message: &str) {
    let panels = app.visible_panels();
    if panels.is_empty() {
        render_hidden_notice(f, area, app, empty_message);
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Fill(1); panels.len()])
        .split(area);
    for (row, panel) in rows.iter().zip(panels) {
        render_panel_content(f, *row, app, panel);
        render_panel_ring(f, *row, app, panel, false);
    }
}

fn render_measure_grid(f: &mut Frame, r: Rect, app: &App) {
    let dense = app.visualization.layout == "dense";
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if dense {
            [Constraint::Fill(1), Constraint::Fill(1)]
        } else {
            [Constraint::Fill(3), Constraint::Fill(2)]
        })
        .split(r);
    let panels = app.visible_panels();
    let (left, right) = if dense {
        (
            panels.iter().step_by(2).copied().collect::<Vec<_>>(),
            panels
                .iter()
                .skip(1)
                .step_by(2)
                .copied()
                .collect::<Vec<_>>(),
        )
    } else {
        (
            panels.iter().take(2).copied().collect::<Vec<_>>(),
            panels.iter().skip(2).copied().collect::<Vec<_>>(),
        )
    };
    let stack = if dense {
        [Constraint::Fill(1), Constraint::Fill(1)]
    } else {
        [Constraint::Fill(3), Constraint::Fill(2)]
    };
    render_panel_stack(
        f,
        columns[0],
        app,
        &left,
        stack,
        "measure panels hidden in setup",
    );
    render_panel_stack(
        f,
        columns[1],
        app,
        &right,
        stack,
        "measure panels hidden in setup",
    );
}

fn render_compact_measure(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let b = panel_block("MEASURE", t);
    f.render_widget(b.clone(), r);
    let ttft = app
        .current
        .as_ref()
        .and_then(|request| request.ttft())
        .map(|duration| format!("{:.0} ms", duration.as_secs_f64() * 1000.0))
        .unwrap_or_else(|| "not measured".into());
    let graph_width = r.width.saturating_sub(24) as usize;
    let decode_scale = app.tok_hist.iter().copied().fold(1.0_f64, f64::max);
    let prefill_scale = app.prefill_hist.iter().copied().fold(1.0_f64, f64::max);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("DECODE  ", Style::default().fg(t.dim)),
            Span::styled(
                format!("{:.1} tok/s", app.real_tg.unwrap_or(0.0)),
                Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
            ),
            Span::styled("    PREFILL  ", Style::default().fg(t.dim)),
            Span::styled(
                format!("{:.0} tok/s", app.real_pp.unwrap_or(0.0)),
                Style::default().fg(t.accent),
            ),
        ]),
        Line::from(format!(
            "TTFT {ttft} | {} completed requests",
            app.spans.len()
        )),
        Line::from(format!(
            "speculation {} | benchmark {}",
            app.metrics
                .draft_acceptance
                .map(|value| format!("{:.1}% accepted", value * 100.0))
                .unwrap_or_else(|| "not reported".into()),
            if app.bench.active { "running" } else { "idle" }
        )),
        Line::from(vec![
            Span::styled("decode history   ", Style::default().fg(t.dim)),
            Span::styled(
                sparkline(
                    &app.tok_hist,
                    graph_width,
                    decode_scale,
                    &app.visualization.graph_renderer,
                ),
                Style::default().fg(t.ok),
            ),
        ]),
        Line::from(vec![
            Span::styled("prefill history  ", Style::default().fg(t.dim)),
            Span::styled(
                sparkline(
                    &app.prefill_hist,
                    graph_width,
                    prefill_scale,
                    &app.visualization.graph_renderer,
                ),
                Style::default().fg(t.accent),
            ),
        ]),
        Line::from(Span::styled(
            app.bench
                .summary
                .as_deref()
                .unwrap_or("No local benchmark result yet."),
            Style::default().fg(t.dim),
        )),
        Line::from(Span::styled(
            "b quick benchmark  r workloads  B context sweep  ? explain",
            Style::default().fg(t.accent),
        )),
    ];
    let inner = b.inner(r);
    if inner.height >= 12 {
        lines.push(Line::from(""));
        if let Some(current) = &app.current {
            lines.push(Line::from(Span::styled(
                format!(
                    "LIVE REQUEST  {} | {} prompt | {} decoded",
                    match current.stage {
                        Stage::Queued => "queued",
                        Stage::Prefill => "prefill",
                        Stage::Decode => "decode",
                        Stage::Done => "done",
                        Stage::Failed => "failed",
                    },
                    current.prompt_tokens,
                    current.decoded
                ),
                Style::default().fg(t.warn),
            )));
        } else if app.spans.is_empty() {
            lines.extend([
                Line::from(Span::styled(
                    "READY WORKLOADS",
                    Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
                )),
                Line::from("Quick response  - short deterministic latency runs"),
                Line::from("Coding turn     - repeatable interactive prompt"),
                Line::from("Long context    - prefill and context sweep"),
                Line::from("Memory soak     - allocator and swap pressure"),
            ]);
        }
        for span in app
            .spans
            .iter()
            .rev()
            .take(inner.height.saturating_sub(lines.len() as u16) as usize)
        {
            lines.push(Line::from(format!(
                "{}  {} prompt -> {} output | ttft {}",
                clip(&span.id, 8),
                span.prompt_tokens,
                span.decoded,
                span.ttft()
                    .map(|duration| format!("{:.0} ms", duration.as_secs_f64() * 1000.0))
                    .unwrap_or_else(|| "-".into())
            )));
        }
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_system(f: &mut Frame, r: Rect, app: &App) {
    if r.height < 7 {
        render_compact_system(f, r, app);
    } else if app.visualization.is_stacked() && r.height >= 28 {
        render_vertical_profile(f, r, app, "system panels hidden in setup");
    } else if r.width >= 76
        && r.height >= 12
        && !app.visualization.is_focused()
        && !app.visualization.is_stacked()
    {
        render_system_grid(f, r, app);
    } else if let Some(panel) = app.selected_panel() {
        render_panel_content(f, r, app, panel);
        render_panel_ring(f, r, app, panel, false);
    } else {
        render_hidden_notice(f, r, app, "system panels hidden in setup");
    }
}

fn render_system_grid(f: &mut Frame, r: Rect, app: &App) {
    let dense = app.visualization.layout == "dense";
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if dense {
            [Constraint::Fill(1), Constraint::Fill(1)]
        } else {
            [Constraint::Fill(3), Constraint::Fill(2)]
        })
        .split(r);
    let panels = app.visible_panels();
    let (left, right) = if dense {
        (
            panels.iter().step_by(2).copied().collect::<Vec<_>>(),
            panels
                .iter()
                .skip(1)
                .step_by(2)
                .copied()
                .collect::<Vec<_>>(),
        )
    } else {
        (
            panels
                .iter()
                .enumerate()
                .filter_map(|(index, panel)| matches!(index, 0 | 3).then_some(*panel))
                .collect::<Vec<_>>(),
            panels
                .iter()
                .enumerate()
                .filter_map(|(index, panel)| matches!(index, 1 | 2).then_some(*panel))
                .collect::<Vec<_>>(),
        )
    };
    let stack = if dense {
        [Constraint::Fill(1), Constraint::Fill(1)]
    } else {
        [Constraint::Fill(3), Constraint::Fill(2)]
    };
    render_panel_stack(
        f,
        columns[0],
        app,
        &left,
        stack,
        "system panels hidden in setup",
    );
    render_panel_stack(
        f,
        columns[1],
        app,
        &right,
        stack,
        "system panels hidden in setup",
    );
}

fn render_hidden_notice(f: &mut Frame, r: Rect, app: &App, message: &str) {
    f.render_widget(
        Paragraph::new(format!("{} | P setup", message)).style(Style::default().fg(app.theme.dim)),
        r,
    );
}

fn render_compact_system(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let findings = app.bloat.findings();
    let storage = app.device.storage();
    let b = panel_block("SYSTEM", t);
    f.render_widget(b.clone(), r);
    let mut lines = vec![
        Line::from(format!(
            "RAM {:.1}/{:.0} GiB used | {:.1} GiB available | swap {:.0} MiB",
            app.rss_gb + app.sys_used_gb,
            app.total_mem_gb,
            app.headroom_gb,
            app.swap_mb
        )),
        Line::from(format!(
            "model disk {:.1} GiB available | {:.0} GiB total",
            storage.available_gib, storage.total_gib
        )),
        Line::from(format!(
            "host CPU {:.0}% | {} endpoints | {} agents detected",
            app.host_cpu_pct,
            app.served.len(),
            app.agents.detected().len()
        )),
        Line::from(format!(
            "Bloat {} | {} finding{}",
            if findings.is_empty() {
                "clear"
            } else {
                "review"
            },
            findings.len(),
            if findings.len() == 1 { "" } else { "s" }
        )),
    ];
    if let Some(offender) = app.interference.offenders.first() {
        lines.push(Line::from(format!(
            "pressure {} | {:.1} GiB RSS | {:.0}% CPU",
            offender.name, offender.mem_gb, offender.cpu
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "No material process contention detected.",
            Style::default().fg(t.ok),
        )));
    }
    if let Some(server) = app.served.first() {
        lines.push(Line::from(format!(
            "{} {} | {}",
            server.runtime,
            server.state,
            server.endpoint_label()
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "No responding inference endpoint.",
            Style::default().fg(t.dim),
        )));
    }
    lines.push(Line::from(Span::styled(
        "m models  c agent setup  6 Bloat evidence  P panel layout",
        Style::default().fg(t.accent),
    )));
    let inner = b.inner(r);
    if inner.height >= 12 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "LOCAL INVENTORY",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!(
            "{} load targets | {} runtime records | {} served endpoints",
            app.server.available.len(),
            app.model_sources.len(),
            app.served.len()
        )));
        for agent in app
            .agents
            .detected()
            .iter()
            .take(inner.height.saturating_sub(lines.len() as u16) as usize)
        {
            lines.push(Line::from(format!(
                "agent {:<14} {}",
                agent.display_name,
                if agent.direct {
                    "direct"
                } else {
                    "proxy required"
                }
            )));
        }
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_bloat(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let findings = app.bloat.findings();
    let safe = findings
        .iter()
        .filter(|finding| finding.can_remove())
        .count();
    let reclaim = findings
        .iter()
        .map(|finding| finding.reclaim_bytes)
        .sum::<u64>();
    let b = panel_block("BLOAT CHECK", t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let mut lines = vec![Line::from(Span::styled(
        if app.bloat.scanning() {
            "QUICK SCAN RUNNING".into()
        } else if findings.is_empty() {
            "CLEAR".into()
        } else {
            format!(
                "{} findings | {} safe | {} reclaimable",
                findings.len(),
                safe,
                bloat::format_bytes(reclaim)
            )
        },
        Style::default()
            .fg(if findings.is_empty() { t.ok } else { t.warn })
            .add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(Span::styled(
        format!(
            "{} | age {}",
            app.bloat.scan_summary(),
            app.bloat
                .scan_age_seconds()
                .map(|seconds| format!("{seconds}s"))
                .unwrap_or_else(|| "not completed".into())
        ),
        Style::default().fg(t.dim),
    )));
    if findings.is_empty() {
        lines.push(Line::from(Span::styled(
            "No runtime or project finding crossed a configured threshold.",
            Style::default().fg(t.dim),
        )));
        lines.push(Line::from(Span::styled(
            "g rescans | 6 opens bounded scan evidence",
            Style::default().fg(t.dim),
        )));
        if inner.height as usize >= lines.len() + 4 {
            lines.extend([
                Line::from(""),
                Line::from(Span::styled(
                    "REMOVAL POLICY",
                    Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
                )),
                Line::from("SAFE is limited to deterministic generated artifacts"),
                Line::from("review findings require evidence and never auto-delete"),
            ]);
        }
    } else {
        lines.push(Line::from(Span::styled(
            "FINDINGS",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        )));
        for finding in findings
            .iter()
            .take((inner.height as usize).saturating_sub(lines.len() + 1))
        {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<7} ", finding.disposition.label()),
                    Style::default().fg(if finding.can_remove() { t.ok } else { t.warn }),
                ),
                Span::styled(
                    clip(&finding.title, inner.width.saturating_sub(9) as usize),
                    Style::default().fg(t.fg),
                ),
            ]));
        }
        lines.push(Line::from(Span::styled(
            "6 evidence and guarded actions | g rescan",
            Style::default().fg(t.dim),
        )));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_bloat_screen(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let findings = app.bloat.findings();
    let safe = findings
        .iter()
        .filter(|finding| finding.can_remove())
        .count();
    let reclaim = findings
        .iter()
        .map(|finding| finding.reclaim_bytes)
        .sum::<u64>();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(r);
    let status = if app.bloat.scanning() {
        format!("QUICK SCAN  running | {}", app.bloat.scan_summary())
    } else {
        format!(
            "QUICK SCAN  {} findings | {} safe | {} reclaimable | {}",
            findings.len(),
            safe,
            bloat::format_bytes(reclaim),
            app.bloat.scan_summary()
        )
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                status,
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "g quick scan | D deep agent scan | j/k select | d twice removes SAFE artifacts",
                Style::default().fg(t.dim),
            )),
        ]),
        rows[0],
    );

    if findings.is_empty() {
        let b = panel_block("BLOAT", t);
        f.render_widget(b.clone(), rows[1]);
        f.render_widget(
            Paragraph::new(if app.bloat.scanning() {
                "Scanning generated artifacts, instruction context, project skills, source concentration, and agent launch gates."
            } else {
                "Clear. Runtime and bounded project checks found no material bloat."
            })
            .style(Style::default().fg(t.dim))
            .wrap(Wrap { trim: true }),
            b.inner(rows[1]),
        );
        return;
    }

    if r.width < 78 || r.height < 14 {
        let selected = app.bloat_sel.min(findings.len() - 1);
        render_bloat_detail(f, rows[1], &findings[selected], app);
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(rows[1]);
    let list_block = panel_block("FINDINGS", t);
    f.render_widget(list_block.clone(), columns[0]);
    let list_area = list_block.inner(columns[0]);
    let visible_rows = list_area.height.max(1) as usize;
    let selected = app.bloat_sel.min(findings.len() - 1);
    let start = selected.saturating_sub(visible_rows.saturating_sub(1));
    let lines = findings
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_rows)
        .map(|(index, finding)| {
            let active = index == selected;
            Line::from(vec![
                Span::styled(
                    if active { "> " } else { "  " },
                    Style::default().fg(if active { t.accent } else { t.dim }),
                ),
                Span::styled(
                    format!("{:<11}", finding.disposition.label()),
                    Style::default().fg(if finding.can_remove() { t.ok } else { t.warn }),
                ),
                Span::styled(
                    clip(&finding.title, list_area.width.saturating_sub(15) as usize),
                    Style::default()
                        .fg(if active { t.fg } else { t.dim })
                        .add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(lines), list_area);
    render_bloat_detail(f, columns[1], &findings[selected], app);
}

fn render_bloat_detail(f: &mut Frame, r: Rect, finding: &bloat::Finding, app: &App) {
    let t = &app.theme;
    let b = panel_block("EVIDENCE", t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                finding.disposition.label(),
                Style::default()
                    .fg(if finding.can_remove() { t.ok } else { t.warn })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}  {}", finding.confidence.label(), finding.code),
                Style::default().fg(t.dim),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            finding.title.clone(),
            Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("evidence  {}", finding.evidence),
            Style::default().fg(t.fg),
        )),
        Line::from(Span::styled(
            format!("next      {}", finding.action),
            Style::default().fg(t.accent),
        )),
    ];
    if finding.reclaim_bytes > 0 {
        lines.push(Line::from(Span::styled(
            format!("reclaim   {}", bloat::format_bytes(finding.reclaim_bytes)),
            Style::default().fg(t.ok),
        )));
    }
    if let Some(path) = finding.relative_path() {
        lines.push(Line::from(Span::styled(
            format!("artifact  {}", path.display()),
            Style::default().fg(t.dim),
        )));
    }
    if app.bloat.removing() {
        lines.push(Line::from(Span::styled(
            "removal in progress",
            Style::default().fg(t.warn),
        )));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_sources(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let b = panel_block("ENDPOINTS / PROVENANCE", t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let mut lines = Vec::new();
    if app.served.is_empty() {
        lines.push(Line::from(Span::styled(
            "No inference endpoint is responding.",
            Style::default().fg(t.dim),
        )));
    } else {
        for server in app
            .served
            .iter()
            .take(inner.height.saturating_sub(3) as usize)
        {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}  ", server.endpoint_label()),
                    Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{}  ", server.runtime), Style::default().fg(t.fg)),
                Span::styled(
                    clip(
                        &public_model_id(&server.model),
                        inner.width.saturating_sub(22) as usize,
                    ),
                    Style::default().fg(t.dim),
                ),
            ]));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("HOST      ", Style::default().fg(t.dim)),
        Span::styled(
            format!(
                "{}{} | {} RAM",
                platform::os_name(),
                if platform::is_omarchy() {
                    " / Omarchy"
                } else {
                    ""
                },
                platform::memory_kind()
            ),
            Style::default().fg(t.fg),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("PROBE     ", Style::default().fg(t.dim)),
        Span::styled(
            app.runtime_observed_at
                .map(|observed| format!("localhost | {}s old", observed.elapsed().as_secs()))
                .unwrap_or_else(|| "no completed sample".into()),
            Style::default().fg(t.fg),
        ),
    ]));
    if inner.height as usize >= lines.len() + app.cfg.telemetry.ports.len() + 3 {
        lines.push(Line::from(Span::styled(
            "CHECKED PORTS",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        )));
        for port in &app.cfg.telemetry.ports {
            let server = app.served.iter().find(|server| server.port == *port);
            lines.push(Line::from(vec![
                Span::styled(format!(":{port:<7}"), Style::default().fg(t.dim)),
                Span::styled(
                    server
                        .map(|server| format!("{} | {}", server.runtime, server.state))
                        .unwrap_or_else(|| "no response".into()),
                    Style::default().fg(if server.is_some() { t.ok } else { t.dim }),
                ),
            ]));
        }
    }
    for (label, present) in [
        ("rates", app.real_tg.is_some() || app.real_pp.is_some()),
        ("speculation", app.metrics.draft_acceptance.is_some()),
        ("allocator", app.metrics.memory_active_bytes.is_some()),
    ] {
        lines.push(Line::from(vec![
            Span::styled(format!("{label:<10}"), Style::default().fg(t.dim)),
            Span::styled(
                if present {
                    "runtime-reported"
                } else {
                    "not reported"
                },
                Style::default().fg(if present { t.ok } else { t.dim }),
            ),
        ]));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn lesson_list_lines<'a>(
    lessons: &'a [learn::Lesson],
    selected: usize,
    t: &Theme,
) -> Vec<Line<'a>> {
    lessons
        .iter()
        .enumerate()
        .map(|(index, lesson)| {
            let active = index == selected;
            Line::from(Span::styled(
                format!("{} {}", if active { ">" } else { " " }, lesson.term),
                Style::default()
                    .fg(if active { t.accent } else { t.dim })
                    .add_modifier(if active {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ))
        })
        .collect()
}

fn lesson_detail_lines<'a>(lesson: &'a learn::Lesson, t: &Theme) -> Vec<Line<'a>> {
    vec![
        Line::from(Span::styled(
            lesson.definition,
            Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("why it matters  {}", lesson.why),
            Style::default().fg(t.fg),
        )),
        Line::from(Span::styled(
            format!("what to watch   {}", lesson.watch),
            Style::default().fg(t.accent),
        )),
        Line::from(Span::styled(
            format!("current reading {}", lesson.current),
            Style::default().fg(t.ok),
        )),
        Line::from(""),
        Line::from(Span::styled(lesson.next, Style::default().fg(t.dim))),
        Line::from(Span::styled(
            "j/k select | m models | b benchmark | 1 overview",
            Style::default().fg(t.dim),
        )),
    ]
}

fn render_lesson_detail(f: &mut Frame, area: Rect, lesson: &learn::Lesson, t: &Theme) {
    let detail_block = panel_block(lesson.term, t);
    f.render_widget(detail_block.clone(), area);
    f.render_widget(
        Paragraph::new(lesson_detail_lines(lesson, t)).wrap(Wrap { trim: true }),
        detail_block.inner(area),
    );
}

fn render_learn_screen(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let lessons = learn::lessons(app);
    let selected = app.learn_sel.min(lessons.len().saturating_sub(1));
    let lesson = &lessons[selected];

    if r.width >= 70 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(r);
        let list_block = panel_block("LEARN", t);
        f.render_widget(list_block.clone(), columns[0]);
        f.render_widget(
            Paragraph::new(lesson_list_lines(&lessons, selected, t)),
            list_block.inner(columns[0]),
        );
        render_lesson_detail(f, columns[1], lesson, t);
    } else if r.height >= 22 {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length((lessons.len() as u16 + 2).min(r.height.saturating_sub(8))),
                Constraint::Min(8),
            ])
            .split(r);
        let list_block = panel_block("LEARN", t);
        f.render_widget(list_block.clone(), rows[0]);
        f.render_widget(
            Paragraph::new(lesson_list_lines(&lessons, selected, t)),
            list_block.inner(rows[0]),
        );
        render_lesson_detail(f, rows[1], lesson, t);
    } else {
        let b = panel_block("LEARN", t);
        f.render_widget(b.clone(), r);
        let topics = lessons
            .iter()
            .enumerate()
            .map(|(index, lesson)| {
                if index == selected {
                    format!("[{}]", lesson.term)
                } else {
                    lesson.term.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("  ");
        let mut lines = vec![Line::from(Span::styled(topics, Style::default().fg(t.dim)))];
        lines.extend(lesson_detail_lines(lesson, t));
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), b.inner(r));
    }
}

fn setup_detail_lines(app: &App) -> Vec<Line<'static>> {
    let t = &app.theme;
    match app.settings_sel {
        0 => vec![
            Line::from(Span::styled(
                "PALETTE",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from("Semantic color roles stay separate from panel and graph layout."),
            Line::from(Span::styled(
                "Enter opens first-party and optional Ghostty palettes.",
                Style::default().fg(t.dim),
            )),
        ],
        1 => vec![
            Line::from(Span::styled(
                "VISUALIZATION PROFILE",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from(format!(
                "{}: {}",
                app.visualization.name, app.visualization.description
            )),
            Line::from(Span::styled(
                "Enter cycles immutable built-ins. Custom TOML uses the typed CLI.",
                Style::default().fg(t.dim),
            )),
        ],
        2 => vec![
            Line::from(Span::styled(
                "DENSITY",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from("Adjusts the active profile after it has supplied a safe default."),
            Line::from(Span::styled(
                "Enter cycles compact, standard, and expanded.",
                Style::default().fg(t.dim),
            )),
        ],
        3 => vec![
            Line::from(Span::styled(
                "START SCREEN",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from("Chooses the first focused screen when Tokoro opens."),
            Line::from(Span::styled(
                "Enter cycles through every screen.",
                Style::default().fg(t.dim),
            )),
        ],
        4 => vec![
            Line::from(Span::styled(
                "VISIBLE PANELS",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from("Controls which evidence panels appear on Measure and System."),
            Line::from(Span::styled(
                "Enter or P opens the panel picker.",
                Style::default().fg(t.dim),
            )),
        ],
        5 => vec![
            Line::from(Span::styled(
                "SIGNAL FOCUS",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from("Chooses the three live signals kept in the compact Measure panel."),
            Line::from(Span::styled(
                "Expanded evidence still shows every collected signal.",
                Style::default().fg(t.dim),
            )),
        ],
        6 => vec![
            Line::from(Span::styled(
                "HISTORY WINDOW",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from("Bounds session-only in-memory samples; no usage telemetry is uploaded."),
            Line::from(Span::styled(
                "Enter cycles 40, 80, and 160 poll slots.",
                Style::default().fg(t.dim),
            )),
        ],
        7 => vec![
            Line::from(Span::styled(
                "REQUEST RETENTION",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from("Keeps a session-only metrics ledger without prompt or response bodies."),
            Line::from(Span::styled(
                "Enter cycles 16, 32, 64, and 128 records.",
                Style::default().fg(t.dim),
            )),
        ],
        8 => vec![
            Line::from(Span::styled(
                "LAUNCH IDENTITY",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from("Shows Cursor Home opening the Threshold during interactive startup."),
            Line::from(Span::styled(
                "Enter turns the launch identity on or off.",
                Style::default().fg(t.dim),
            )),
        ],
        9 => vec![
            Line::from(Span::styled(
                "LAUNCH MOTION",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from(
                "Full selects six cells; reduced uses two frames; none shows the resolved mark.",
            ),
            Line::from(Span::styled(
                "Enter cycles full, reduced, and none.",
                Style::default().fg(t.dim),
            )),
        ],
        10 => vec![
            Line::from(Span::styled(
                "LAUNCH SOUND",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from(
                "The built-in ident uses six dry selections, one latch, and a widening interval.",
            ),
            Line::from(Span::styled(
                "Enter toggles the opt-in sound. Playback failure stays silent.",
                Style::default().fg(t.dim),
            )),
        ],
        _ => vec![
            Line::from(Span::styled(
                "QUICK WALKTHROUGH",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from("Choose a goal and Tokoro opens the right starting view."),
            Line::from(Span::styled(
                "Three short steps. Esc or S skips from anywhere.",
                Style::default().fg(t.dim),
            )),
        ],
    }
}

fn render_customize(f: &mut Frame, r: Rect, app: &App) {
    let t = &app.theme;
    let theme_value = if app.cfg.theme.name.is_empty() {
        format!("auto / {} terminal", platform::os_name())
    } else {
        app.cfg.theme.name.clone()
    };
    let intro_value = if app.cfg.intro.enabled { "on" } else { "off" };
    let sound_value = if matches!(
        app.cfg.intro.sound.as_str(),
        "" | "off" | "tokoro" | "freedom"
    ) {
        app.cfg.intro.sound.as_str()
    } else {
        "custom"
    };
    let history_value = format!("{} poll slots", app.cfg.observability.history_samples());
    let request_value = format!(
        "{} metrics-only records",
        app.cfg.observability.request_retention()
    );
    let settings = [
        ("palette", theme_value.as_str()),
        ("profile", app.visualization.name.as_str()),
        ("density", app.cfg.layout.density.as_str()),
        ("home", app.cfg.layout.default_view.as_str()),
        ("panels", "choose visible evidence"),
        ("signals", app.cfg.observability.focus()),
        ("history", history_value.as_str()),
        ("requests", request_value.as_str()),
        ("launch", intro_value),
        ("motion", app.cfg.intro.motion.as_str()),
        ("sound", sound_value),
        ("walkthrough", "open three-step guide"),
    ];
    let mut list_lines = vec![Line::from(Span::styled(
        "OPEN-SOURCE ALPHA · NO USAGE TELEMETRY",
        Style::default().fg(t.dim),
    ))];
    for (index, (name, value)) in settings.iter().enumerate() {
        let selected = index == app.settings_sel;
        list_lines.push(Line::from(vec![
            Span::styled(
                if selected { "> " } else { "  " },
                Style::default().fg(if selected { t.accent } else { t.dim }),
            ),
            Span::styled(
                format!("{name:<10}"),
                Style::default()
                    .fg(if selected { t.fg } else { t.dim })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            Span::styled(*value, Style::default().fg(t.fg)),
        ]));
    }
    list_lines.push(Line::from(Span::styled(
        "j/k select  Enter change  P panels  1 overview",
        Style::default().fg(t.dim),
    )));

    let (list_area, detail_area) = if r.width >= 72 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
            .split(r);
        (columns[0], Some(columns[1]))
    } else if r.height >= 18 {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(16), Constraint::Min(5)])
            .split(r);
        (rows[0], Some(rows[1]))
    } else {
        (r, None)
    };

    let list_block = panel_block("SETUP", t);
    f.render_widget(list_block.clone(), list_area);
    if detail_area.is_none() {
        list_lines.extend(setup_detail_lines(app));
    }
    f.render_widget(
        Paragraph::new(list_lines).wrap(Wrap { trim: true }),
        list_block.inner(list_area),
    );
    if let Some(detail_area) = detail_area {
        let detail_block = panel_block("SELECTED", t);
        f.render_widget(detail_block.clone(), detail_area);
        f.render_widget(
            Paragraph::new(setup_detail_lines(app)).wrap(Wrap { trim: true }),
            detail_block.inner(detail_area),
        );
    }
}

fn render_onboarding_popup(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let popup_area = if area.width >= 72 && area.height >= 20 {
        centered(area, 74, 72)
    } else {
        area
    };
    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .title(format!(
            " QUICK WALKTHROUGH | {}/3 | ESC OR S SKIPS ",
            app.onboarding_step + 1
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));
    f.render_widget(block.clone(), popup_area);
    let inner = block.inner(popup_area);

    let mut lines = vec![Line::from(vec![
        Span::styled(
            "TOKORO",
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" / OPEN-SOURCE ALPHA", Style::default().fg(t.dim)),
    ])];

    match app.onboarding_step {
        0 => {
            lines.extend([
                Line::from(""),
                Line::from(Span::styled(
                    "A place for local models.",
                    Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("Discover -> Choose -> Run -> Connect -> Understand"),
                Line::from(""),
                Line::from("Tokoro is checking this machine while you read."),
                Line::from(Span::styled(
                    "No account. No usage telemetry. Prompts and responses stay local.",
                    Style::default().fg(t.dim),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Enter", Style::default().fg(t.accent)),
                    Span::raw(" continue   "),
                    Span::styled("Esc / S", Style::default().fg(t.dim)),
                    Span::raw(" skip"),
                ]),
            ]);
        }
        1 => {
            lines.extend([
                Line::from(""),
                Line::from(Span::styled(
                    "What do you want to do first?",
                    Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "This only chooses where the walkthrough leaves you.",
                    Style::default().fg(t.dim),
                )),
                Line::from(""),
            ]);
            let show_details = inner.height >= 16;
            for (index, choice) in ONBOARDING_CHOICES.iter().enumerate() {
                let selected = index == app.onboarding_sel;
                lines.push(Line::from(vec![
                    Span::styled(
                        if selected { "> " } else { "  " },
                        Style::default().fg(if selected { t.accent } else { t.dim }),
                    ),
                    Span::styled(
                        format!("{}  {}", index + 1, choice.label),
                        Style::default()
                            .fg(if selected { t.fg } else { t.dim })
                            .add_modifier(if selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                ]));
                if show_details {
                    lines.push(Line::from(Span::styled(
                        format!("     {}", choice.detail),
                        Style::default().fg(t.dim),
                    )));
                }
            }
            lines.extend([
                Line::from(""),
                Line::from(vec![
                    Span::styled("Up / Down", Style::default().fg(t.accent)),
                    Span::raw(" choose   "),
                    Span::styled("Enter", Style::default().fg(t.accent)),
                    Span::raw(" continue"),
                ]),
            ]);
        }
        _ => {
            let choice = &ONBOARDING_CHOICES[app
                .onboarding_sel
                .min(ONBOARDING_CHOICES.len().saturating_sub(1))];
            lines.extend([
                Line::from(""),
                Line::from(Span::styled(
                    "You are ready.",
                    Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
                )),
                Line::from(vec![
                    Span::styled("Start in  ", Style::default().fg(t.dim)),
                    Span::styled(
                        choice.destination,
                        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(choice.detail),
                Line::from(""),
            ]);
            if inner.height >= 18 {
                lines.extend([
                    Line::from("Up / Down or j / k   choose"),
                    Line::from("Enter                open"),
                    Line::from("Tab                  move between visible panels"),
                    Line::from("Esc                  go back"),
                    Line::from("?                    open Learn"),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Reopen this guide from Setup or the command palette.",
                        Style::default().fg(t.dim),
                    )),
                    Line::from(""),
                ]);
            } else {
                lines.extend([
                    Line::from("Up/Down choose  Enter open  Tab panels"),
                    Line::from("Esc back  ? Learn"),
                    Line::from(""),
                ]);
            }
            lines.push(Line::from(vec![
                Span::styled("Enter", Style::default().fg(t.accent)),
                Span::raw(format!(" open {}", choice.destination)),
            ]));
        }
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_command_popup(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let r = workspace_popup(area);
    f.render_widget(Clear, r);
    let b = Block::default()
        .title(" COMMANDS | TYPE TO FILTER | ENTER RUN | ESC CLOSE ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let items = commands::catalog();
    let matches = commands::matches(&app.command_query);
    let mut lines = vec![Line::from(Span::styled(
        format!("/ {}", app.command_query),
        Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
    ))];
    let visible = inner.height.saturating_sub(2) as usize;
    let start = visible_start(app.command_sel, matches.len(), visible);
    for (row, index) in matches.iter().enumerate().skip(start).take(visible) {
        let item = &items[*index];
        let selected = row == app.command_sel;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}{} ", if selected { ">" } else { " " }, item.key),
                Style::default().fg(if selected { t.accent } else { t.dim }),
            ),
            Span::styled(
                format!("{:<22}", item.label),
                Style::default()
                    .fg(if selected { t.fg } else { t.dim })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            Span::styled(item.detail, Style::default().fg(t.dim)),
        ]));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_themes_popup(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let r = centered(area, 64, 78);
    f.render_widget(Clear, r);
    let b = Block::default()
        .title(" THEMES | TYPE FILTER | J/K SELECT | ENTER APPLY | ESC CLOSE ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);
    f.render_widget(
        Paragraph::new(format!("filter: {}", app.theme_query)).style(Style::default().fg(t.fg)),
        rows[0],
    );
    let matches = theme_matches(app);
    let visible = rows[1].height.max(1) as usize;
    let selected = app.popup_sel.min(matches.len().saturating_sub(1));
    let start = selected.saturating_sub(visible.saturating_sub(1));
    let lines = matches
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .map(|(row, index)| {
            let active = row == selected;
            let name = &app.theme_choices[*index];
            let current = if app.cfg.theme.name.is_empty() {
                name == "auto"
            } else {
                name == &app.cfg.theme.name
            };
            Line::from(vec![
                Span::styled(
                    if active { "> " } else { "  " },
                    Style::default().fg(if active { t.accent } else { t.dim }),
                ),
                Span::styled(if current { "* " } else { "  " }, Style::default().fg(t.ok)),
                Span::styled(
                    name.clone(),
                    Style::default()
                        .fg(if active { t.fg } else { t.dim })
                        .add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(lines), rows[1]);
}

fn render_panels_popup(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let r = centered(area, 70, 78);
    f.render_widget(Clear, r);
    let b = Block::default()
        .title(" PANELS | SPACE TOGGLE | S SAVE | ESC CLOSE ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let lines = app
        .cfg
        .layout
        .panels
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let selected = index == app.popup_sel;
            let visible = app.cfg.layout.panel_visible(name);
            Line::from(vec![
                Span::styled(
                    format!(
                        "{}{} ",
                        if selected { ">" } else { " " },
                        if visible { "[x]" } else { "[ ]" }
                    ),
                    Style::default().fg(if selected { t.accent } else { t.dim }),
                ),
                Span::styled(
                    panel_label(name),
                    Style::default()
                        .fg(if selected { t.fg } else { t.dim })
                        .add_modifier(if selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_memory(f: &mut Frame, r: Rect, app: &App, t: &Theme) {
    let b = panel_block("MEMORY STACK", t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let used = app.rss_gb + app.sys_used_gb;
    let used_fraction = used / app.total_mem_gb.max(1.0);
    let meter_width = inner.width.saturating_sub(24).clamp(8, 34) as usize;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!(
                    "{} RAM {:.1}/{:.0} GiB  ",
                    platform::memory_kind(),
                    used,
                    app.total_mem_gb
                ),
                Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
            ),
            Span::styled(bar(used_fraction, meter_width), Style::default().fg(t.warn)),
        ]),
        Line::from(vec![
            Span::styled("server RSS  ", Style::default().fg(t.dim)),
            Span::styled(format!("{:.1} GiB", app.rss_gb), Style::default().fg(t.fg)),
            Span::styled(
                format!("  OS/apps {:.1} GiB", app.sys_used_gb),
                Style::default().fg(t.dim),
            ),
        ]),
    ];
    if platform::has_unified_memory() {
        lines.extend([Line::from(vec![
            Span::styled("weights     ", Style::default().fg(t.dim)),
            Span::styled(
                format!(
                    "{}{:.1} GiB",
                    if app.real_vram_gb.is_some() { "" } else { "~" },
                    app.weights_gb
                ),
                Style::default().fg(t.weights),
            ),
            Span::styled(
                format!(
                    "  KV/other {}{:.1} GiB",
                    if app.metrics.kv_cache_tokens.is_some() {
                        ""
                    } else {
                        "~"
                    },
                    app.kv_gb
                ),
                Style::default().fg(t.kv),
            ),
        ])]);
    } else {
        lines.push(Line::from(vec![
            Span::styled("device model ", Style::default().fg(t.dim)),
            Span::styled(
                app.real_vram_gb
                    .map(|value| format!("{value:.1} GiB reported; outside host-RSS accounting"))
                    .unwrap_or_else(|| "not reported; not added to system RAM".into()),
                Style::default().fg(t.weights),
            ),
        ]));
    }
    lines.extend([
        Line::from(vec![
            Span::styled("headroom    ", Style::default().fg(t.dim)),
            Span::styled(
                format!("{:.1} GiB", app.headroom_gb),
                Style::default().fg(if app.headroom_gb > 20.0 { t.ok } else { t.err }),
            ),
            Span::styled(
                format!("  swap {:.0} MiB", app.swap_mb),
                Style::default().fg(if app.swap_mb < 50.0 { t.dim } else { t.warn }),
            ),
        ]),
        Line::from(vec![
            Span::styled("context     ", Style::default().fg(t.dim)),
            Span::styled(
                bar(
                    app.ceiling.current_tokens as f64 / app.ceiling.effective_max().max(1) as f64,
                    meter_width,
                ),
                Style::default().fg(t.kv),
            ),
            Span::styled(
                format!(
                    "  {}k/{}k",
                    app.ceiling.current_tokens / 1000,
                    app.ceiling.effective_max() / 1000
                ),
                Style::default().fg(t.fg),
            ),
        ]),
    ]);
    if let Some(active) = app.metrics.memory_active_bytes {
        lines.push(Line::from(Span::styled(
            format!(
                "allocator {:.1} GiB active | {:.1} peak | {:.1} cache",
                active as f64 / BYTES_PER_GIB,
                app.metrics.memory_peak_bytes.unwrap_or(active) as f64 / BYTES_PER_GIB,
                app.metrics.memory_cache_bytes.unwrap_or(0) as f64 / BYTES_PER_GIB
            ),
            Style::default().fg(t.dim),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "allocator not reported by the responding runtime",
            Style::default().fg(t.dim),
        )));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_performance(f: &mut Frame, r: Rect, app: &App, t: &Theme) {
    let b = panel_block("PERFORMANCE / SPECULATION", t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let cur = app.current.as_ref();
    let round_rate = app
        .latest_round
        .as_ref()
        .and_then(|round| (round.ms > 0.0).then(|| round.committed as f64 / (round.ms / 1000.0)));
    let live_decode = cur
        .map(|request| request.decode_rate)
        .filter(|rate| *rate > 0.0)
        .or(round_rate);
    let decode = live_decode.or(app.real_tg).unwrap_or(0.0);
    let prefill = cur
        .map(|request| request.prefill_rate)
        .filter(|rate| *rate > 0.0)
        .or(app.real_pp)
        .unwrap_or(0.0);
    let hit = cur.and_then(|c| c.cache_hit_ratio());
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "decode  {}{:.1} tok/s",
                if live_decode.is_some() || app.real_tg.is_some() {
                    ""
                } else {
                    "~"
                },
                decode
            ),
            Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(
                "prefill {}{:.0} tok/s",
                if cur
                    .map(|request| request.prefill_rate > 0.0)
                    .unwrap_or(false)
                    || app.real_pp.is_some()
                {
                    ""
                } else {
                    "~"
                },
                prefill
            ),
            Style::default().fg(t.accent),
        )),
        Line::from(Span::styled(
            format!(
                "ttft    {}",
                cur.and_then(|c| c.ttft())
                    .map(|d| format!("{:.0} ms", d.as_secs_f64() * 1000.0))
                    .unwrap_or_else(|| "-".into())
            ),
            Style::default().fg(t.warn),
        )),
        Line::from(Span::styled(
            format!("cpu     {:.0}%", app.cpu_pct),
            Style::default().fg(t.fg),
        )),
        Line::from(Span::styled(
            format!(
                "source  {} | {}",
                if cur.is_some() {
                    "live request"
                } else if app.latest_round.is_some() {
                    "latest verified round"
                } else if app.real_tg.is_some() || app.real_pp.is_some() {
                    "runtime report"
                } else {
                    "no measured sample"
                },
                app.runtime_observed_at
                    .map(|observed| format!("{}s old", observed.elapsed().as_secs()))
                    .unwrap_or_else(|| "age unknown".into())
            ),
            Style::default().fg(t.dim),
        )),
    ];
    if let Some(accept) = app.metrics.mean_accept_len {
        let mode = app
            .metrics
            .mode
            .as_deref()
            .map(|mode| format!(" | {}", mode))
            .unwrap_or_default();
        lines.push(Line::from(Span::styled(
            format!("accept  {:.2} tok/round{}", accept, mode),
            Style::default().fg(t.kv),
        )));
    }
    if let Some(rate) = app.metrics.draft_acceptance {
        let rounds = app.metrics.rounds.unwrap_or(0);
        lines.push(Line::from(Span::styled(
            format!("draft   {:.1}% accepted | {} rounds", rate * 100.0, rounds),
            Style::default().fg(t.kv),
        )));
    }
    if !app.metrics.position_acceptance.is_empty() {
        let curve = app
            .metrics
            .position_acceptance
            .iter()
            .take(8)
            .map(|value| format!("{:.0}%", value * 100.0))
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(Line::from(Span::styled(
            format!("d0..  {}", curve),
            Style::default().fg(t.kv),
        )));
    }
    if let Some(round) = &app.latest_round {
        lines.push(Line::from(Span::styled(
            format!(
                "round   {} committed / {} drafted / {} accepted | {:.1} ms | cap {} | {}",
                round.committed,
                round.drafted,
                round.accepted,
                round.ms,
                round
                    .cap
                    .map(|cap| cap.to_string())
                    .unwrap_or_else(|| "-".into()),
                if round.source.is_empty() {
                    "target"
                } else {
                    &round.source
                }
            ),
            Style::default().fg(t.fg),
        )));
    }
    if let Some(hits) = app.metrics.prefix_hits {
        lines.push(Line::from(Span::styled(
            format!(
                "prefix  {} hits | {} partial | {} reused tok",
                hits,
                app.metrics.prefix_partial_hits.unwrap_or(0),
                app.metrics.prefix_reused_tokens.unwrap_or(0)
            ),
            Style::default().fg(t.kv),
        )));
    }
    if let Some(h) = hit {
        lines.push(Line::from(Span::styled(
            format!(
                "request cache {:.0}% ({}/{})",
                h * 100.0,
                cur.and_then(|request| request.cached_tokens).unwrap_or(0),
                cur.map(|request| request.prompt_tokens).unwrap_or(0)
            ),
            Style::default().fg(t.kv),
        )));
    }
    if app.metrics.kv_cache_usage.is_some()
        || app.metrics.requests_running.is_some()
        || app.metrics.requests_waiting.is_some()
    {
        lines.push(Line::from(Span::styled(
            format!(
                "sched   run {} | wait {} | swap {} | KV {}",
                app.metrics.requests_running.unwrap_or(0),
                app.metrics.requests_waiting.unwrap_or(0),
                app.metrics.requests_swapped.unwrap_or(0),
                app.metrics
                    .kv_cache_usage
                    .map(|usage| format!("{:.0}%", usage * 100.0))
                    .unwrap_or_else(|| "not reported".into())
            ),
            Style::default().fg(if app.metrics.requests_waiting.unwrap_or(0) > 0 {
                t.warn
            } else {
                t.dim
            }),
        )));
    }
    if let Some(max) = app.metrics.batch_max {
        lines.push(Line::from(Span::styled(
            format!(
                "batch   max {} | {} batches | {} requests",
                max,
                app.metrics.batch_batches.unwrap_or(0),
                app.metrics.batch_requests.unwrap_or(0)
            ),
            Style::default().fg(t.dim),
        )));
    }
    if app.bench.active {
        let progress = if app.bench.concurrency {
            format!(
                "concurrency sweep c{} ({}/{})",
                app.bench
                    .concurrency_levels
                    .get(app.bench.concurrency_idx)
                    .copied()
                    .unwrap_or(1),
                app.bench.concurrency_idx + 1,
                app.bench.concurrency_levels.len()
            )
        } else if app.bench.sweep {
            format!(
                "benchmark sweep {}/{}",
                app.bench.sweep_idx + 1,
                app.bench.sweep_sizes.len()
            )
        } else {
            format!("benchmark run {}/{}", app.bench.run + 1, app.bench.runs)
        };
        lines.push(Line::from(Span::styled(
            progress,
            Style::default().fg(t.warn),
        )));
    } else if let Some(s) = &app.bench.summary {
        lines.push(Line::from(Span::styled(
            s.clone(),
            Style::default().fg(t.ok),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "r recipes | b quick run | B prompt sweep",
            Style::default().fg(t.dim),
        )));
    }
    if inner.height as usize >= lines.len() + 5 {
        lines.extend([
            Line::from(""),
            Line::from(Span::styled(
                "REQUEST OUTCOME",
                Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
            )),
            Line::from(format!("{} retained requests", app.spans.len())),
            Line::from(format!(
                "{} runs | {} concurrency points | percentiles {}",
                app.bench.results.len(),
                app.bench.concurrency_results.len(),
                if app.bench.results.len() >= 3 || !app.bench.concurrency_results.is_empty() {
                    "available"
                } else {
                    "need 3 runs"
                }
            )),
            Line::from(Span::styled(
                "Speculative acceptance is shown only when the runtime reports it.",
                Style::default().fg(t.dim),
            )),
        ]);
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_stages(f: &mut Frame, r: Rect, app: &App, t: &Theme) {
    let b = panel_block("INFERENCE PATH", t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let mut lines = Vec::new();
    if let Some(last) = app.selected_request() {
        let total = (last.last_update - last.started).as_secs_f64().max(0.001);
        let to_first_s = last
            .ttft()
            .map(|duration| duration.as_secs_f64())
            .unwrap_or(total);
        let decode_s = (total - to_first_s).max(0.0);
        let width = inner.width.saturating_sub(23).max(8) as usize;
        let cached = last.cached_tokens.unwrap_or(0).min(last.prompt_tokens);
        let fresh = last.prompt_tokens.saturating_sub(cached);
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}  ", clip(&last.id, 10)),
                Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{} -> {} tok | {}",
                    last.prompt_tokens,
                    last.decoded,
                    stage_name(last.stage)
                ),
                Style::default().fg(t.dim),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("to first ", Style::default().fg(t.accent)),
            Span::styled(
                bar(to_first_s / total, width),
                Style::default().fg(t.accent),
            ),
            Span::styled(
                format!(" {:.0}ms", to_first_s * 1000.0),
                Style::default().fg(t.fg),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("decode   ", Style::default().fg(t.ok)),
            Span::styled(bar(decode_s / total, width), Style::default().fg(t.ok)),
            Span::styled(
                format!(" {:.0}ms", decode_s * 1000.0),
                Style::default().fg(t.fg),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            format!(
                "prompt {cached} cached + {fresh} fresh | prefill {:.0} tok/s",
                last.prefill_rate
            ),
            Style::default().fg(t.kv),
        )));
        lines.push(Line::from(Span::styled(
            format!("sampling {}", last.sampling_summary()),
            Style::default().fg(t.dim),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "scheduler now: run {} | wait {} | swap {} | KV {}",
                app.metrics
                    .requests_running
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "?".into()),
                app.metrics
                    .requests_waiting
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "?".into()),
                app.metrics
                    .requests_swapped
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "?".into()),
                app.metrics
                    .kv_cache_usage
                    .map(|usage| format!("{:.0}%", usage * 100.0))
                    .unwrap_or_else(|| "?".into())
            ),
            Style::default().fg(t.dim),
        )));
        if inner.height as usize > lines.len() {
            lines.push(Line::from(Span::styled(
                format!(
                    "verify {} | prefix {} | total {:.1}s",
                    app.metrics
                        .draft_acceptance
                        .map(|value| format!("{:.0}% accepted", value * 100.0))
                        .unwrap_or_else(|| "not reported".into()),
                    prefix_reuse_summary(app),
                    total
                ),
                Style::default().fg(t.dim),
            )));
        }
        if inner.height as usize > lines.len() {
            lines.push(Line::from(Span::styled(
                format!("KV runtime {}", kv_residency_summary(app)),
                Style::default().fg(t.dim),
            )));
        }
    } else {
        lines.extend([
            Line::from(Span::styled(
                "No request has been observed.",
                Style::default().fg(t.dim),
            )),
            Line::from(""),
            Line::from("input -> queue -> prefill -> first token -> decode -> finish"),
            Line::from("cache reuse and speculative verification appear only when reported"),
            Line::from("unknown stages remain '?' rather than becoming estimates"),
            Line::from(Span::styled(
                "b runs a request | r chooses its shape",
                Style::default().fg(t.accent),
            )),
        ]);
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_history(f: &mut Frame, r: Rect, app: &App, t: &Theme) {
    let b = panel_block("REQUEST HISTORY", t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let requests = app.spans.iter().rev().collect::<Vec<_>>();
    let selected_id = app.selected_request().map(|request| request.id.as_str());
    let selected_index = selected_id
        .and_then(|id| requests.iter().position(|request| request.id == id))
        .unwrap_or(0);
    let visible = inner.height as usize;
    let start = visible_start(selected_index, requests.len(), visible);
    let mut lines: Vec<Line> = requests
        .into_iter()
        .skip(start)
        .take(visible)
        .map(|request| {
            let (mark, color) = match request.stage {
                Stage::Done => ("OK", t.ok),
                Stage::Failed => ("ERR", t.err),
                _ => ("..", t.warn),
            };
            let selected = selected_id == Some(request.id.as_str());
            Line::from(vec![
                Span::styled(
                    if selected { "> " } else { "  " },
                    Style::default().fg(if selected { t.accent } else { t.dim }),
                ),
                Span::styled(format!("{} ", mark), Style::default().fg(color)),
                Span::styled(
                    format!(
                        "{} {} -> {} tok",
                        clip(&request.id, 8),
                        request.prompt_tokens,
                        request.decoded
                    ),
                    Style::default().fg(t.fg).add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
                Span::styled(
                    format!(
                        "  ttft {}",
                        request
                            .ttft()
                            .map(|duration| format!("{:.1}s", duration.as_secs_f64()))
                            .unwrap_or_else(|| "-".into())
                    ),
                    Style::default().fg(t.dim),
                ),
            ])
        })
        .collect();
    if lines.is_empty() {
        lines.extend([
            Line::from(Span::styled(
                "No request has completed in this local session.",
                Style::default().fg(t.dim),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "b quick benchmark",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from("r choose a workload-shaped recipe"),
            Line::from("B sweep prompt sizes"),
            Line::from(""),
            Line::from(Span::styled(
                "LEDGER CONTRACT",
                Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
            )),
            Line::from("selection follows request identity, not a changing row index"),
            Line::from("prompt and response bodies are excluded"),
            Line::from(format!(
                "up to {} local request records are retained",
                app.cfg.observability.request_retention()
            )),
        ]);
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_interference(f: &mut Frame, r: Rect, app: &App, t: &Theme) {
    let memory_pressure = app.headroom_gb < 20.0 || app.swap_mb > 100.0;
    let has_warnings = !app.interference.warnings.is_empty();
    let b = panel_block("SYSTEM PRESSURE", t).border_style(Style::default().fg(if has_warnings {
        t.warn
    } else {
        t.dim
    }));
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);

    let mut lines: Vec<Line> = vec![Line::from(vec![
        Span::styled(
            if app.interference.paused {
                "PAUSED  "
            } else {
                "LIVE  "
            },
            Style::default().fg(if app.interference.paused {
                t.warn
            } else {
                t.ok
            }),
        ),
        Span::styled(
            format!(
                "host CPU {:.0}% | {:.1} GiB RAM free | swap {:.0} MiB",
                app.host_cpu_pct, app.headroom_gb, app.swap_mb
            ),
            Style::default().fg(t.dim),
        ),
    ])];
    if app.interference.offenders.is_empty() && !has_warnings {
        lines.extend([
            Line::from(Span::styled(
                "clear - no material process contention",
                Style::default().fg(t.ok),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "PROBLEM-FIRST FILTER",
                Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
            )),
            Line::from("shows outside processes above 0.5 GiB RSS or 25% CPU"),
            Line::from("Tokoro, terminals, shells, and system services are protected"),
            Line::from("the process list can pause while host totals remain live"),
        ]);
    } else {
        let selected = app
            .interference
            .selected
            .min(app.interference.offenders.len().saturating_sub(1));
        let reserved = 1usize + app.interference.warnings.len();
        let visible = (inner.height as usize)
            .saturating_sub(lines.len() + reserved)
            .max(1);
        let start = visible_start(selected, app.interference.offenders.len(), visible);
        let name_width = inner.width.saturating_sub(22).clamp(8, 18) as usize;
        for (index, offender) in app
            .interference
            .offenders
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
        {
            let active = index == selected;
            let impact = if memory_pressure && offender.mem_gb > 1.0 {
                "MEM"
            } else if offender.cpu > 30.0 {
                "CPU"
            } else {
                "LOW"
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if active { "> " } else { "  " },
                    Style::default().fg(if active { t.accent } else { t.dim }),
                ),
                Span::styled(
                    format!(
                        "{:<width$}",
                        clip(&offender.name, name_width),
                        width = name_width
                    ),
                    Style::default()
                        .fg(if active { t.fg } else { t.dim })
                        .add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(
                    format!(" {:>4.1}G {:>3.0}% {impact}", offender.mem_gb, offender.cpu),
                    Style::default().fg(if impact == "LOW" { t.dim } else { t.warn }),
                ),
            ]));
        }
        for warning in &app.interference.warnings {
            lines.push(Line::from(Span::styled(
                format!("! {warning}"),
                Style::default().fg(t.warn),
            )));
        }
        if let Some(offender) = app.interference.offenders.get(selected) {
            lines.push(Line::from(Span::styled(
                format!("selected PID {} | x twice to terminate", offender.pid),
                Style::default().fg(t.dim),
            )));
        }
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_streams(f: &mut Frame, r: Rect, app: &App, t: &Theme) {
    let focus = app.cfg.observability.focus();
    let b = panel_block("INFERENCE SIGNALS", t);
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let width = inner.width.saturating_sub(20).max(8) as usize;
    let latest = |history: &VecDeque<f64>| history.back().copied().unwrap_or(0.0);
    let dynamic_scale = |history: &VecDeque<f64>| history.iter().copied().fold(1.0_f64, f64::max);
    let mut lines = Vec::new();
    let mut signal = |label: &str,
                      value: String,
                      history: &VecDeque<f64>,
                      scale: f64,
                      color: ratatui::style::Color| {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{label:<8}{value:>8}  "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                sparkline(history, width, scale, &app.visualization.graph_renderer),
                Style::default().fg(color),
            ),
        ]));
    };

    match focus {
        "latency" => {
            signal(
                "TTFT",
                format!("{:.0} ms", latest(&app.ttft_hist)),
                &app.ttft_hist,
                dynamic_scale(&app.ttft_hist),
                t.warn,
            );
            signal(
                "decode",
                format!("{:.1} t/s", latest(&app.tok_hist)),
                &app.tok_hist,
                dynamic_scale(&app.tok_hist),
                t.ok,
            );
            signal(
                "waiting",
                format!("{:.0} req", latest(&app.queue_hist)),
                &app.queue_hist,
                dynamic_scale(&app.queue_hist),
                t.err,
            );
        }
        "throughput" => {
            signal(
                "decode",
                format!("{:.1} t/s", latest(&app.tok_hist)),
                &app.tok_hist,
                dynamic_scale(&app.tok_hist),
                t.ok,
            );
            signal(
                "prefill",
                format!("{:.0} t/s", latest(&app.prefill_hist)),
                &app.prefill_hist,
                dynamic_scale(&app.prefill_hist),
                t.accent,
            );
            signal(
                "waiting",
                format!("{:.0} req", latest(&app.queue_hist)),
                &app.queue_hist,
                dynamic_scale(&app.queue_hist),
                t.err,
            );
        }
        "memory" => {
            signal(
                "KV use",
                format!("{:.0}%", latest(&app.kv_hist)),
                &app.kv_hist,
                100.0,
                t.kv,
            );
            signal(
                "waiting",
                format!("{:.0} req", latest(&app.queue_hist)),
                &app.queue_hist,
                dynamic_scale(&app.queue_hist),
                t.err,
            );
            signal(
                "engine",
                format!("{:.0}%", latest(&app.load_hist)),
                &app.load_hist,
                100.0,
                t.warn,
            );
        }
        "speculation" => {
            signal(
                "accept",
                format!("{:.0}%", latest(&app.acceptance_hist)),
                &app.acceptance_hist,
                100.0,
                t.kv,
            );
            signal(
                "decode",
                format!("{:.1} t/s", latest(&app.tok_hist)),
                &app.tok_hist,
                dynamic_scale(&app.tok_hist),
                t.ok,
            );
            signal(
                "KV use",
                format!("{:.0}%", latest(&app.kv_hist)),
                &app.kv_hist,
                100.0,
                t.accent,
            );
        }
        _ => {
            signal(
                "decode",
                format!("{:.1} t/s", latest(&app.tok_hist)),
                &app.tok_hist,
                dynamic_scale(&app.tok_hist),
                t.ok,
            );
            signal(
                "TTFT",
                format!("{:.0} ms", latest(&app.ttft_hist)),
                &app.ttft_hist,
                dynamic_scale(&app.ttft_hist),
                t.warn,
            );
            signal(
                "KV use",
                format!("{:.0}%", latest(&app.kv_hist)),
                &app.kv_hist,
                100.0,
                t.kv,
            );
        }
    }

    lines.push(Line::from(Span::styled(
        format!(
            "focus {focus} | {} session slots | independent scales",
            app.cfg.observability.history_samples()
        ),
        Style::default().fg(t.dim),
    )));
    if inner.height as usize > lines.len() + 2 {
        lines.extend([
            Line::from(Span::styled(
                "CURRENT SCHEDULER",
                Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
            )),
            Line::from(format!(
                "running {} | waiting {} | swapped {}",
                app.metrics.requests_running.unwrap_or(0),
                app.metrics.requests_waiting.unwrap_or(0),
                app.metrics.requests_swapped.unwrap_or(0)
            )),
            Line::from(Span::styled(
                "Enter opens every tracked signal and its provenance.",
                Style::default().fg(t.dim),
            )),
        ]);
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_publish_popup(f: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let r = centered(area, 80, 72);
    f.render_widget(Clear, r);
    let b = Block::default()
        .title(" EXPORT | 1 COPY MARKDOWN | 2 SAVE EDITABLE PACK | 3 COPY JSON | ESC CLOSE ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let report = report::benchmark_markdown(app);
    let lines = vec![
        Line::from(Span::styled(
            "REDACTED PREVIEW",
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Paths, prompts, responses, PIDs, and secrets are excluded.",
            Style::default().fg(t.dim),
        )),
        Line::from(Span::styled(
            "2 saves bundle.json + editable report.toml + Markdown + CSV",
            Style::default().fg(t.fg),
        )),
        Line::from(Span::styled(
            "Measured data is SHA-256 checked; recipes change presentation only.",
            Style::default().fg(t.fg),
        )),
        Line::from(Span::styled(
            "After saving, `tokoro handoff list` prepares verified sharing packs.",
            Style::default().fg(t.accent),
        )),
        Line::from(""),
    ];
    let mut preview = lines;
    preview.extend(
        report
            .lines()
            .take(inner.height.saturating_sub(7) as usize)
            .map(|line| Line::from(Span::styled(line.to_string(), Style::default().fg(t.dim)))),
    );
    f.render_widget(Paragraph::new(preview).wrap(Wrap { trim: false }), inner);
}

fn render_benchmarks_popup(f: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let r = centered(area, 78, 70);
    f.render_widget(Clear, r);
    let b = Block::default()
        .title(" BENCHMARKS | J/K SELECT | ENTER RUN | ESC CLOSE ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let recipes = benchmark_recipes(app);
    let mut lines = vec![Line::from(Span::styled(
        "LOCAL ONLY | temperature 0 | server-reported rates preferred",
        Style::default().fg(t.dim),
    ))];
    for (index, recipe) in recipes.iter().enumerate() {
        let selected = index == app.popup_sel;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", if selected { ">" } else { " " }),
                Style::default().fg(if selected { t.accent } else { t.dim }),
            ),
            Span::styled(
                format!("{:<18}", recipe.name),
                Style::default()
                    .fg(if selected { t.fg } else { t.dim })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            Span::styled(recipe.description.clone(), Style::default().fg(t.dim)),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "Results stay local until an explicit export preview.",
        Style::default().fg(t.dim),
    )));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_models_popup(f: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let r = workspace_popup(area);
    f.render_widget(Clear, r);
    let b = Block::default()
        .title(" MODELS | 1 LOCAL  2 HUGGING FACE  3 LOCAL.AI | ESC CLOSE ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let tab = |label: &'static str, active: bool| {
        Span::styled(
            label,
            Style::default()
                .fg(if active { t.fg } else { t.dim })
                .add_modifier(if active {
                    Modifier::BOLD | Modifier::UNDERLINED
                } else {
                    Modifier::empty()
                }),
        )
    };
    let storage = app.device.storage();
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                tab("1 LOCAL", app.model_tab == ModelTab::Local),
                Span::raw("    "),
                tab("2 HUGGING FACE", app.model_tab == ModelTab::HuggingFace),
                Span::raw("    "),
                tab("3 LOCAL.AI", app.model_tab == ModelTab::LocalAi),
            ]),
            Line::from(Span::styled(
                format!(
                    "{} | {:.0} GiB RAM | {:.1} GiB model disk free",
                    app.chip, app.total_mem_gb, storage.available_gib
                ),
                Style::default().fg(t.dim),
            )),
        ]),
        rows[0],
    );

    match app.model_tab {
        ModelTab::Local => render_local_models(f, rows[1], app, t),
        ModelTab::HuggingFace => render_huggingface_models(f, rows[1], app, t),
        ModelTab::LocalAi => render_local_ai_models(f, rows[1], app, t),
    }
    let footer = match app.model_tab {
        ModelTab::Local => "j/k select  Enter load  2 Hugging Face  3 sourced comparison",
        ModelTab::HuggingFace => {
            "j/k select  Enter download/load  r recheck  s small starters  1 local"
        }
        ModelTab::LocalAi => {
            "j/k select  f find exact HF artifacts  Enter copy note  r refresh  h starters"
        }
    };
    f.render_widget(
        Paragraph::new(footer).style(Style::default().fg(t.accent)),
        rows[2],
    );
}

fn model_columns(area: Rect) -> Option<(Rect, Rect)> {
    if area.width < 68 {
        return None;
    }
    let list_width = (area.width * 2 / 5).clamp(24, 38);
    if area.width.saturating_sub(list_width) < 36 {
        return None;
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(list_width), Constraint::Min(36)])
        .split(area);
    Some((columns[0], columns[1]))
}

fn render_local_models(f: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let selected = app
        .popup_sel
        .min(app.server.available.len().saturating_sub(1));
    let mut details = vec![Line::from(Span::styled(
        if app.online {
            format!(
                "LIVE  {} :{}  {}",
                app.engine,
                app.port,
                public_model_id(&app.model)
            )
        } else {
            "IDLE  no model loaded".into()
        },
        Style::default()
            .fg(if app.online { t.ok } else { t.dim })
            .add_modifier(Modifier::BOLD),
    ))];
    if let Some(server) = app.served.iter().find(|server| server.port == app.port) {
        details.push(Line::from(Span::styled(
            format!(
                "owner {} | target {}",
                server.owner.as_deref().unwrap_or("not reported"),
                server.target.as_deref().unwrap_or("not reported")
            ),
            Style::default().fg(t.dim),
        )));
    }
    if app.server.catalog_loading() {
        details.push(Line::from(Span::styled(
            "runtime catalog scan in progress",
            Style::default().fg(t.warn),
        )));
    }
    if let Some(choice) = app.server.available.get(selected) {
        details.extend([
            Line::from(""),
            Line::from(Span::styled(
                choice.label.clone(),
                Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                choice.detail.clone(),
                Style::default().fg(t.dim),
            )),
            Line::from(Span::styled(
                if choice.can_start {
                    "Enter loads this target. Downloads may occur if the runtime catalog says so."
                } else {
                    "Blocked because the runtime reports that it exceeds RAM."
                },
                Style::default().fg(if choice.can_start { t.accent } else { t.warn }),
            )),
        ]);
    } else {
        details.push(Line::from("No local load target was found."));
    }
    let installed = app
        .model_sources
        .iter()
        .filter(|source| source.state == "installed")
        .count();
    details.push(Line::from(""));
    details.push(Line::from(Span::styled(
        format!(
            "{} served endpoint{} | {} installed runtime model{} | {} load target{}",
            app.served.len(),
            if app.served.len() == 1 { "" } else { "s" },
            installed,
            if installed == 1 { "" } else { "s" },
            app.server.available.len(),
            if app.server.available.len() == 1 {
                ""
            } else {
                "s"
            }
        ),
        Style::default().fg(t.dim),
    )));

    let Some((left, right)) = model_columns(area) else {
        f.render_widget(Paragraph::new(details).wrap(Wrap { trim: true }), area);
        return;
    };
    let list_block = panel_block("LOAD TARGETS", t);
    f.render_widget(list_block.clone(), left);
    let list_area = list_block.inner(left);
    let visible = list_area.height.max(1) as usize;
    let start = visible_start(selected, app.server.available.len(), visible);
    let lines = app
        .server
        .available
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .map(|(index, choice)| {
            let active = index == selected;
            Line::from(vec![
                Span::styled(
                    if active { "> " } else { "  " },
                    Style::default().fg(if active { t.accent } else { t.dim }),
                ),
                Span::styled(
                    clip(&choice.label, list_area.width.saturating_sub(4) as usize),
                    Style::default()
                        .fg(if active { t.fg } else { t.dim })
                        .add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(lines), list_area);
    let detail_block = panel_block("CURRENT / SELECTED", t);
    f.render_widget(detail_block.clone(), right);
    f.render_widget(
        Paragraph::new(details).wrap(Wrap { trim: true }),
        detail_block.inner(right),
    );
}

fn render_huggingface_models(f: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let entries = app.huggingface.entries();
    let selected = app.popup_sel.min(entries.len().saturating_sub(1));
    let Some(entry) = entries.get(selected) else {
        f.render_widget(
            Paragraph::new("No curated Hugging Face starter is configured."),
            area,
        );
        return;
    };
    let mut details = vec![
        Line::from(Span::styled(
            entry.label.as_str(),
            Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            entry.repo.as_str(),
            Style::default().fg(t.accent),
        )),
        Line::from(Span::styled(
            entry.purpose.as_str(),
            Style::default().fg(t.dim),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("STATE   ", Style::default().fg(t.dim)),
            Span::styled(
                entry.state_label(),
                Style::default().fg(if entry.installed() { t.ok } else { t.fg }),
            ),
        ]),
        Line::from(Span::styled(entry.detail(), Style::default().fg(t.dim))),
    ];
    if let Some(manifest) = entry.manifest() {
        details.extend([
            Line::from(Span::styled(
                format!("PINNED  {}", manifest.revision),
                Style::default().fg(t.dim),
            )),
            Line::from(Span::styled(
                format!(
                    "FILES   {} files | {}",
                    manifest.files.len(),
                    huggingface::format_bytes(manifest.total_bytes)
                ),
                Style::default().fg(t.dim),
            )),
            Line::from(Span::styled(
                "CHECKS  public, ungated, safetensors, LFS SHA-256",
                Style::default().fg(t.ok),
            )),
        ]);
    } else {
        details.push(Line::from(Span::styled(
            "Press r to resolve the repository and pin its current commit.",
            Style::default().fg(t.accent),
        )));
    }
    if app.huggingface.busy() {
        let progress = app.huggingface.progress();
        let text = if progress.total_bytes > 0 {
            format!(
                "{}  {} / {}",
                app.huggingface.operation().unwrap_or("working"),
                huggingface::format_bytes(progress.downloaded_bytes),
                huggingface::format_bytes(progress.total_bytes)
            )
        } else {
            app.huggingface.operation().unwrap_or("working").to_string()
        };
        details.push(Line::from(Span::styled(text, Style::default().fg(t.warn))));
    } else if entry.manifest().is_some() {
        details.push(Line::from(Span::styled(
            if entry.installed() {
                "Enter loads the local copy."
            } else {
                "Enter downloads to a staging directory, verifies the weights, then installs."
            },
            Style::default().fg(t.accent),
        )));
    }

    let Some((left, right)) = model_columns(area) else {
        f.render_widget(Paragraph::new(details).wrap(Wrap { trim: true }), area);
        return;
    };
    let list_block = panel_block(app.huggingface.heading(), t);
    f.render_widget(list_block.clone(), left);
    let list_area = list_block.inner(left);
    let visible = list_area.height.max(1) as usize;
    let start = visible_start(selected, entries.len(), visible);
    let lines = entries
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .map(|(index, candidate)| {
            let active = index == selected;
            Line::from(vec![
                Span::styled(
                    if active { "> " } else { "  " },
                    Style::default().fg(if active { t.accent } else { t.dim }),
                ),
                Span::styled(
                    clip(&candidate.label, list_area.width.saturating_sub(4) as usize),
                    Style::default()
                        .fg(if active { t.fg } else { t.dim })
                        .add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(lines), list_area);
    let detail_block = panel_block("VERIFIED DOWNLOAD", t);
    f.render_widget(detail_block.clone(), right);
    f.render_widget(
        Paragraph::new(details).wrap(Wrap { trim: true }),
        detail_block.inner(right),
    );
}

fn render_local_ai_models(f: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let reading = app.local_ai.reading_for(&app.chip, app.total_mem_gb);
    let recommendations = reading
        .map(|reading| reading.recommendations.as_slice())
        .unwrap_or_default();
    let selected = app.popup_sel.min(recommendations.len().saturating_sub(1));
    let mut details = Vec::new();
    if let (Some(reading), Some(recommendation)) = (reading, recommendations.get(selected)) {
        let source_method = if reading.source_method.is_empty() {
            "cached public-web extraction"
        } else {
            &reading.source_method
        };
        details.extend([
            Line::from(Span::styled(
                "PUBLIC LOCAL.AI RECOMMENDATION",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!(
                    "{} | {:.0} GB | {}",
                    reading.machine, reading.memory_gb, source_method
                ),
                Style::default().fg(t.dim),
            )),
            Line::from(""),
            Line::from(Span::styled(
                recommendation.label.clone(),
                Style::default().fg(t.dim),
            )),
            Line::from(Span::styled(
                recommendation.model.clone(),
                Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
            )),
            Line::from(format!(
                "intelligence {} | speed {} tasks/hr | source size {} GB",
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
                    .unwrap_or_else(|| "not reported".into())
            )),
            Line::from(format!(
                "this device: {:.1} GiB RAM available | {:.1} GiB model disk available",
                app.headroom_gb,
                app.device.storage().available_gib
            )),
            Line::from(Span::styled(
                "Source size excludes runtime and context memory. Local measurement decides fit.",
                Style::default().fg(t.dim),
            )),
            Line::from(Span::styled(
                "External comparison, not a Tokoro benchmark. Prove performance locally.",
                Style::default().fg(t.warn),
            )),
            Line::from(Span::styled(
                "Press f to find matching Hugging Face artifacts, or Enter to copy the source note.",
                Style::default().fg(t.accent),
            )),
        ]);
    } else {
        details.extend([
            Line::from(Span::styled(
                "NO CACHED LOCAL.AI RECOMMENDATION",
                Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!(
                    "public-web search adapter: {}{}",
                    app.local_ai.adapter_label(),
                    app.local_ai
                        .last_error()
                        .map(|error| format!(" | {error}"))
                        .unwrap_or_default()
                ),
                Style::default().fg(t.dim),
            )),
            Line::from(""),
            Line::from("No account is needed. This source is optional."),
            Line::from(Span::styled(
                "Press h to use deterministic Hugging Face manifests now.",
                Style::default().fg(t.accent),
            )),
        ]);
    }

    if model_columns(area).is_none() && details.len() > 7 {
        details.truncate(7);
        details.push(Line::from(Span::styled(
            "External source, not measured here.",
            Style::default().fg(t.warn),
        )));
        details.push(Line::from(Span::styled(
            "f finds matching HF artifacts. Enter copies the source note. h opens starters.",
            Style::default().fg(t.accent),
        )));
    }
    let Some((left, right)) = model_columns(area) else {
        f.render_widget(Paragraph::new(details).wrap(Wrap { trim: true }), area);
        return;
    };
    let list_block = panel_block("SOURCE RESULTS", t);
    f.render_widget(list_block.clone(), left);
    let list_area = list_block.inner(left);
    let visible = list_area.height.max(1) as usize;
    let start = visible_start(selected, recommendations.len(), visible);
    let lines = recommendations
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .map(|(index, recommendation)| {
            let active = index == selected;
            Line::from(vec![
                Span::styled(
                    if active { "> " } else { "  " },
                    Style::default().fg(if active { t.accent } else { t.dim }),
                ),
                Span::styled(
                    clip(
                        &format!("{}  {}", recommendation.label, recommendation.model),
                        list_area.width.saturating_sub(4) as usize,
                    ),
                    Style::default()
                        .fg(if active { t.fg } else { t.dim })
                        .add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(lines), list_area);
    let detail_block = panel_block("PROVENANCE / FIT", t);
    f.render_widget(detail_block.clone(), right);
    f.render_widget(
        Paragraph::new(details).wrap(Wrap { trim: true }),
        detail_block.inner(right),
    );
}

fn render_connect_popup(f: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let r = workspace_popup(area);
    f.render_widget(Clear, r);
    let snippets = harness_snippets(&app.connect_model, connection_port(app));
    let matches = connection_matches(app);
    let b = Block::default()
        .title(" AGENTS | J/K SELECT | ENTER COPY SETUP | M MODEL | ESC CLOSE ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);

    let columns = if inner.width >= 68 {
        let list_width = (inner.width / 3).clamp(24, 32);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(list_width), Constraint::Min(36)])
            .split(inner);
        Some((columns[0], columns[1]))
    } else {
        None
    };
    let preview_area = columns.map(|(_, preview)| preview).unwrap_or(inner);

    if let Some((list_area, _)) = columns {
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(list_area);
        f.render_widget(
            Paragraph::new(format!(
                "{} found | filter: {}",
                app.agents.detected().len(),
                app.connect_query
            )),
            left[0],
        );
        let visible = left[1].height.max(1) as usize;
        let start = visible_start(app.popup_sel, matches.len(), visible);
        let list: Vec<Line> = matches
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .map(|(row, index)| {
                let name = snippets[*index].0;
                let selected = row == app.popup_sel;
                let favorite = if app.connect_favorites.contains(name) {
                    "*"
                } else {
                    " "
                };
                let detected = app
                    .agents
                    .detected()
                    .iter()
                    .find(|agent| agent.snippet_name == name);
                let found = match detected {
                    Some(agent) if agent.direct => "[D]",
                    Some(_) => "[P]",
                    None => "[ ]",
                };
                Line::from(vec![
                    Span::styled(
                        format!(
                            "{} {}{} ",
                            if selected { ">" } else { " " },
                            found,
                            favorite
                        ),
                        Style::default().fg(if detected.is_some() { t.ok } else { t.dim }),
                    ),
                    Span::styled(
                        name,
                        Style::default()
                            .fg(if selected { t.fg } else { t.dim })
                            .add_modifier(if selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(list), left[1]);
    }

    let selected = matches.get(app.popup_sel).copied();
    let mut preview = vec![
        Line::from(Span::styled(
            format!(
                "{} :{} | model {}",
                if app.online { "LIVE" } else { "PREPARED" },
                connection_port(app),
                app.connect_model
            ),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            if app.online {
                "The endpoint is responding now."
            } else {
                "Start a model before using the copied setup."
            },
            Style::default().fg(if app.online { t.ok } else { t.warn }),
        )),
    ];
    if let Some(index) = selected {
        let (name, snippet) = &snippets[index];
        let detection = app
            .agents
            .detected()
            .iter()
            .find(|agent| agent.snippet_name == *name);
        preview.push(Line::from(Span::styled(
            match detection {
                Some(agent) if agent.direct => format!(
                    "{} detected | {} | direct OpenAI-compatible setup",
                    agent.display_name, agent.evidence
                ),
                Some(agent) => format!(
                    "{} detected | {} | local proxy required",
                    agent.display_name, agent.evidence
                ),
                None => format!("{} not detected | setup can still be copied", name),
            },
            Style::default().fg(if detection.is_some() { t.ok } else { t.dim }),
        )));
        preview.push(Line::from(Span::styled(
            connection_description(name),
            Style::default().fg(t.fg),
        )));
        preview.push(Line::from(""));
        preview.extend(snippet.lines().map(|line| Line::from(line.to_string())));
    } else {
        preview.push(Line::from(Span::styled(
            "no connection matches the filter",
            Style::default().fg(t.warn),
        )));
    }
    f.render_widget(
        Paragraph::new(preview)
            .style(Style::default().fg(t.fg))
            .wrap(Wrap { trim: false }),
        preview_area,
    );
}

fn render_connect_models_popup(f: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let r = centered(area, 70, 72);
    f.render_widget(Clear, r);
    let b = Block::default()
        .title(" CONNECTION MODEL | J/K SELECT | ENTER USE | ESC BACK ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));
    f.render_widget(b.clone(), r);
    let inner = b.inner(r);
    let choices = connection_model_choices(app);
    let lines = choices
        .iter()
        .enumerate()
        .map(|(index, model)| {
            let selected = index == app.popup_sel;
            Line::from(Span::styled(
                format!("{} {}", if selected { ">" } else { " " }, model),
                Style::default()
                    .fg(if selected { t.fg } else { t.dim })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ))
        })
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}
