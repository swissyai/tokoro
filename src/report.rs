use super::{infer_parameter_billions, infer_quantization, platform, public_model_id, App};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) const REPORT_SCHEMA: &str = "tokoro.report.v1";
pub(crate) const RECIPE_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ReportEnvelope {
    pub schema: String,
    pub sha256: String,
    pub data: ReportData,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ReportData {
    pub captured_unix: u64,
    pub environment: Environment,
    pub model: ModelIdentity,
    pub workload: Workload,
    pub runs: Vec<RunReading>,
    pub sweep: Vec<SweepReading>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub concurrency: Vec<ConcurrencyReading>,
    #[serde(default, skip_serializing_if = "BenchmarkPressure::is_empty")]
    pub benchmark_pressure: BenchmarkPressure,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub budgets: Vec<BudgetAssessment>,
    pub provenance: Provenance,
    pub privacy: PrivacyReceipt,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Environment {
    pub hardware: String,
    pub unified_memory_gib: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub memory_kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub platform: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub os_version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tokoro_version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ModelIdentity {
    pub id: String,
    pub parameters: String,
    pub quantization: String,
    pub engine: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub engine_version: String,
    pub mode: String,
    pub context_limit_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Workload {
    pub name: String,
    pub prompt_tokens: u32,
    pub output_limit_tokens: u32,
    pub requested_runs: u32,
    pub temperature: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub concurrency_levels: Vec<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RunReading {
    pub run: usize,
    pub prefill_tokens_per_second: f64,
    pub decode_tokens_per_second: f64,
    pub ttft_milliseconds: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub time_per_output_token_milliseconds: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub end_to_end_milliseconds: f64,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub output_tokens: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token_count_source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SweepReading {
    pub prompt_tokens: u32,
    pub prefill_tokens_per_second: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ConcurrencyReading {
    pub concurrency: u32,
    pub completed: u32,
    pub errors: u32,
    pub wall_milliseconds: f64,
    pub system_tokens_per_second: f64,
    pub mean_request_tokens_per_second: f64,
    pub p95_latency_milliseconds: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub p95_time_per_output_token_milliseconds: f64,
    pub peak_waiting_requests: Option<u64>,
    pub peak_kv_cache_usage: Option<f64>,
    pub peak_server_rss_gib: f64,
    pub peak_swap_mib: f64,
    pub min_headroom_gib: f64,
    pub token_count_source: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct BenchmarkPressure {
    pub peak_server_rss_gib: f64,
    pub peak_swap_mib: f64,
    pub min_headroom_gib: Option<f64>,
    pub peak_waiting_requests: Option<u64>,
    pub peak_kv_cache_usage: Option<f64>,
}

impl BenchmarkPressure {
    fn is_empty(&self) -> bool {
        self.peak_server_rss_gib == 0.0
            && self.peak_swap_mib == 0.0
            && self.min_headroom_gib.is_none()
            && self.peak_waiting_requests.is_none()
            && self.peak_kv_cache_usage.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct BudgetAssessment {
    pub metric: String,
    pub status: String,
    pub relation: String,
    pub target: f64,
    pub observed: Option<f64>,
    pub unit: String,
    pub evidence: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Provenance {
    pub rates: String,
    pub timing: String,
    pub environment: String,
    pub custody: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PrivacyReceipt {
    pub excluded: Vec<String>,
    pub prompts_included: bool,
    pub responses_included: bool,
    pub absolute_paths_included: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ReportRecipe {
    pub version: u32,
    pub title: String,
    pub subtitle: String,
    pub sections: ReportSections,
    pub narrative: ReportNarrative,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ReportSections {
    pub summary: bool,
    pub environment: bool,
    pub runs: bool,
    pub sweep: bool,
    pub methodology: bool,
    pub provenance: bool,
    pub privacy: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct ReportNarrative {
    pub context: String,
    pub conclusion: String,
}

impl Default for ReportRecipe {
    fn default() -> Self {
        Self {
            version: RECIPE_VERSION,
            title: "Tokoro local inference result".into(),
            subtitle: "Reproducible local benchmark".into(),
            sections: ReportSections {
                summary: true,
                environment: true,
                runs: true,
                sweep: true,
                methodology: true,
                provenance: true,
                privacy: true,
            },
            narrative: ReportNarrative::default(),
        }
    }
}

pub(crate) fn budget_assessments(app: &App) -> Vec<BudgetAssessment> {
    let Some(budget) = app.cfg.benchmark.budget_for(&app.bench.label) else {
        return Vec::new();
    };
    let mut assessments = Vec::new();
    let mut add = |metric: &str,
                   relation: &str,
                   target: f64,
                   observed: Option<f64>,
                   unit: &str,
                   evidence: String| {
        let status = match observed {
            Some(value) if relation == "at_most" && value <= target => "pass",
            Some(value) if relation == "at_least" && value >= target => "pass",
            Some(_) => "breach",
            None => "unavailable",
        };
        assessments.push(BudgetAssessment {
            metric: metric.into(),
            status: status.into(),
            relation: relation.into(),
            target: round_one(target),
            observed: observed.map(round_one),
            unit: unit.into(),
            evidence,
        });
    };

    if let Some(target) = budget.max_ttft_p95_ms {
        let observed = (app.bench.results.len() >= 3)
            .then(|| percentile(app.bench.results.iter().map(|run| run.ttft_ms), 0.95))
            .flatten();
        add(
            "ttft_p95",
            "at_most",
            target,
            observed,
            "ms",
            if observed.is_some() {
                format!("{} comparable sequential runs", app.bench.results.len())
            } else {
                "three comparable sequential runs required".into()
            },
        );
    }
    if let Some(target) = budget.max_tpot_p95_ms {
        let observed = (app.bench.results.len() >= 3)
            .then(|| percentile(app.bench.results.iter().map(|run| run.tpot_ms), 0.95))
            .flatten()
            .or_else(|| {
                app.bench
                    .concurrency_results
                    .iter()
                    .find(|run| run.concurrency == 1 && run.errors == 0)
                    .map(|run| run.p95_tpot_ms)
            });
        add(
            "tpot_p95",
            "at_most",
            target,
            observed,
            "ms/token",
            "derived from measured decode duration and server usage tokens when available".into(),
        );
    }
    if let Some(target) = budget.max_end_to_end_p95_ms {
        let observed = if app.bench.results.len() >= 3 {
            percentile(app.bench.results.iter().map(|run| run.end_to_end_ms), 0.95)
        } else {
            app.bench
                .concurrency_results
                .iter()
                .find(|run| run.concurrency == 1 && run.errors == 0)
                .map(|run| run.p95_latency_ms)
        };
        add(
            "end_to_end_p95",
            "at_most",
            target,
            observed,
            "ms",
            "request completion measured by Tokoro; concurrency uses the c1 point".into(),
        );
    }
    if let Some(target) = budget.min_decode_tokens_per_second {
        let observed = if app.bench.results.is_empty() {
            app.bench
                .concurrency_results
                .iter()
                .find(|run| run.concurrency == 1 && run.errors == 0)
                .map(|run| run.mean_request_tokens_per_second)
        } else {
            Some(
                app.bench.results.iter().map(|run| run.tg).sum::<f64>()
                    / app.bench.results.len() as f64,
            )
        };
        add(
            "decode_rate",
            "at_least",
            target,
            observed,
            "tok/s",
            "mean per-request decode; concurrency uses the c1 point".into(),
        );
    }
    if let Some(target) = budget.min_system_tokens_per_second {
        let best = app
            .bench
            .concurrency_results
            .iter()
            .filter(|run| run.errors == 0)
            .max_by(|left, right| {
                left.system_tokens_per_second
                    .total_cmp(&right.system_tokens_per_second)
            });
        add(
            "system_throughput",
            "at_least",
            target,
            best.map(|run| run.system_tokens_per_second),
            "tok/s",
            best.map(|run| format!("best error-free point at concurrency {}", run.concurrency))
                .unwrap_or_else(|| "an error-free concurrency point is required".into()),
        );
    }
    if let Some(target) = budget.max_server_rss_gib {
        add(
            "server_rss_peak",
            "at_most",
            target,
            (app.bench.peak_server_rss_gib > 0.0).then_some(app.bench.peak_server_rss_gib),
            "GiB",
            "peak process RSS sampled during this benchmark".into(),
        );
    }
    if let Some(target) = budget.max_swap_mib {
        add(
            "swap_peak",
            "at_most",
            target,
            Some(app.bench.peak_swap_mib),
            "MiB",
            "peak host swap sampled during this benchmark".into(),
        );
    }
    if let Some(target) = budget.max_waiting_requests {
        add(
            "waiting_requests_peak",
            "at_most",
            target as f64,
            app.bench.peak_waiting_requests.map(|value| value as f64),
            "requests",
            "runtime-reported scheduler queue; unavailable when the runtime omits it".into(),
        );
    }
    assessments
}

pub(crate) fn capture(app: &App) -> Result<ReportEnvelope, String> {
    let quantization = app
        .real_quant
        .clone()
        .or_else(|| infer_quantization(&app.model))
        .unwrap_or_else(|| "unknown".into());
    let parameters = app
        .real_params
        .clone()
        .or_else(|| infer_parameter_billions(&app.model).map(|value| format!("{value:.0}B")))
        .unwrap_or_else(|| "unknown".into());
    let data = ReportData {
        captured_unix: app.bench.started_unix,
        environment: Environment {
            hardware: app.chip.clone(),
            unified_memory_gib: round_one(app.total_mem_gb),
            memory_kind: platform::memory_kind().into(),
            platform: platform::os_name().into(),
            os_version: platform::os_version(),
            tokoro_version: env!("CARGO_PKG_VERSION").into(),
        },
        model: ModelIdentity {
            id: public_model_id(&app.model),
            parameters,
            quantization,
            engine: app.engine.clone(),
            engine_version: app.metrics.runtime_version.clone().unwrap_or_default(),
            mode: app.metrics.mode.clone().unwrap_or_else(|| "unknown".into()),
            context_limit_tokens: app.ceiling.model_max as u64,
        },
        workload: Workload {
            name: if app.bench.label.is_empty() {
                "not run".into()
            } else {
                app.bench.label.clone()
            },
            prompt_tokens: app.bench.prompt_tokens,
            output_limit_tokens: app.bench.gen_tokens,
            requested_runs: app.bench.runs,
            temperature: 0.0,
            concurrency_levels: app.bench.concurrency_levels.clone(),
        },
        runs: app
            .bench
            .results
            .iter()
            .enumerate()
            .map(|(index, run)| RunReading {
                run: index + 1,
                prefill_tokens_per_second: round_one(run.pp),
                decode_tokens_per_second: round_one(run.tg),
                ttft_milliseconds: run.ttft_ms.round(),
                time_per_output_token_milliseconds: round_one(run.tpot_ms),
                end_to_end_milliseconds: run.end_to_end_ms.round(),
                output_tokens: run.output_tokens,
                token_count_source: run.token_count_source.clone(),
            })
            .collect(),
        sweep: app
            .bench
            .sweep_results
            .iter()
            .map(|(tokens, rate)| SweepReading {
                prompt_tokens: *tokens,
                prefill_tokens_per_second: round_one(*rate),
            })
            .collect(),
        concurrency: app
            .bench
            .concurrency_results
            .iter()
            .map(|run| ConcurrencyReading {
                concurrency: run.concurrency,
                completed: run.completed,
                errors: run.errors,
                wall_milliseconds: run.wall_ms.round(),
                system_tokens_per_second: round_one(run.system_tokens_per_second),
                mean_request_tokens_per_second: round_one(run.mean_request_tokens_per_second),
                p95_latency_milliseconds: run.p95_latency_ms.round(),
                p95_time_per_output_token_milliseconds: round_one(run.p95_tpot_ms),
                peak_waiting_requests: run.peak_waiting_requests,
                peak_kv_cache_usage: run
                    .peak_kv_cache_usage
                    .map(|value| round_one(value * 100.0)),
                peak_server_rss_gib: round_one(run.peak_server_rss_gib),
                peak_swap_mib: run.peak_swap_mib.round(),
                min_headroom_gib: round_one(run.min_headroom_gib),
                token_count_source: run.token_count_source.clone(),
            })
            .collect(),
        benchmark_pressure: BenchmarkPressure {
            peak_server_rss_gib: round_one(app.bench.peak_server_rss_gib),
            peak_swap_mib: app.bench.peak_swap_mib.round(),
            min_headroom_gib: app.bench.min_headroom_gib.map(round_one),
            peak_waiting_requests: app.bench.peak_waiting_requests,
            peak_kv_cache_usage: app
                .bench
                .peak_kv_cache_usage
                .map(|value| round_one(value * 100.0)),
        },
        budgets: budget_assessments(app),
        provenance: Provenance {
            rates: "server-reported when available; system throughput uses server usage tokens or a labeled stream-frame estimate"
                .into(),
            timing: "TTFT and end-to-end measured by Tokoro; TPOT derives from decode duration and counted output tokens".into(),
            environment: "sampled locally at capture time".into(),
            custody: "local-only until an explicit export".into(),
        },
        privacy: PrivacyReceipt {
            excluded: vec![
                "absolute paths".into(),
                "usernames and contact details".into(),
                "process names and identifiers".into(),
                "prompts and responses".into(),
                "secrets and local configuration".into(),
            ],
            prompts_included: false,
            responses_included: false,
            absolute_paths_included: false,
        },
    };
    envelope(data)
}

fn envelope(data: ReportData) -> Result<ReportEnvelope, String> {
    let canonical = serde_json::to_vec(&data).map_err(|error| error.to_string())?;
    let sha256 = format!("{:x}", Sha256::digest(canonical));
    Ok(ReportEnvelope {
        schema: REPORT_SCHEMA.into(),
        sha256,
        data,
    })
}

pub(crate) fn verify(envelope: &ReportEnvelope) -> Result<(), String> {
    if envelope.schema != REPORT_SCHEMA {
        return Err(format!("unsupported report schema '{}'", envelope.schema));
    }
    let expected = self_hash(&envelope.data)?;
    if expected != envelope.sha256 {
        return Err("report bundle SHA-256 does not match its measured data".into());
    }
    Ok(())
}

fn self_hash(data: &ReportData) -> Result<String, String> {
    let canonical = serde_json::to_vec(data).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(canonical)))
}

pub(crate) fn recipe_toml(recipe: &ReportRecipe) -> Result<String, String> {
    toml::to_string_pretty(recipe).map_err(|error| error.to_string())
}

pub(crate) fn parse_recipe(value: &str) -> Result<ReportRecipe, String> {
    let recipe = toml::from_str::<ReportRecipe>(value).map_err(|error| error.to_string())?;
    if recipe.version != RECIPE_VERSION {
        return Err(format!(
            "unsupported report recipe version {}; expected {}",
            recipe.version, RECIPE_VERSION
        ));
    }
    Ok(recipe)
}

pub(crate) fn render_markdown(
    envelope: &ReportEnvelope,
    recipe: &ReportRecipe,
) -> Result<String, String> {
    verify(envelope)?;
    let data = &envelope.data;
    let mut output = format!("# {}\n", clean_narrative(&recipe.title));
    if !recipe.subtitle.trim().is_empty() {
        output.push_str(&format!("\n{}\n", clean_narrative(&recipe.subtitle)));
    }
    output.push_str(&format!(
        "\n`{}` | bundle `{}`\n",
        envelope.schema,
        &envelope.sha256[..12]
    ));
    if !recipe.narrative.context.trim().is_empty() {
        output.push_str(&format!(
            "\n{}\n",
            clean_narrative(&recipe.narrative.context)
        ));
    }
    if recipe.sections.summary {
        output.push_str("\n## Summary\n\n");
        output.push_str(&format!(
            "- model: {}\n- workload: {}\n- completed runs: {}\n- context limit: {} tokens\n",
            data.model.id,
            data.workload.name,
            data.runs.len(),
            data.model.context_limit_tokens
        ));
        if let Some((prefill, decode, ttft)) = means(&data.runs) {
            output.push_str(&format!(
                "- mean prefill: {prefill:.1} tok/s\n- mean decode: {decode:.1} tok/s\n- mean TTFT: {ttft:.0} ms\n"
            ));
        }
        if let Some(best) = data
            .concurrency
            .iter()
            .filter(|run| run.errors == 0)
            .max_by(|left, right| {
                left.system_tokens_per_second
                    .total_cmp(&right.system_tokens_per_second)
            })
        {
            output.push_str(&format!(
                "- best error-free system throughput: {:.1} tok/s at concurrency {}\n",
                best.system_tokens_per_second, best.concurrency
            ));
        }
        for budget in &data.budgets {
            output.push_str(&format!(
                "- budget {}: {}{}\n",
                budget.metric,
                budget.status,
                budget
                    .observed
                    .map(|value| format!(" ({value:.1} {} vs {:.1})", budget.unit, budget.target))
                    .unwrap_or_else(|| " (measurement unavailable)".into())
            ));
        }
    }
    if recipe.sections.environment {
        output.push_str("\n## Environment\n\n");
        let memory_kind = if data.environment.memory_kind.is_empty() {
            "host"
        } else {
            &data.environment.memory_kind
        };
        output.push_str(&format!(
            "- platform: {}\n- OS version: {}\n- Tokoro version: {}\n- hardware: {} / {:.1} GiB {} memory\n- model: {}\n- parameters: {}\n- quantization: {}\n- engine: {}\n- engine version: {}\n- mode: {}\n",
            if data.environment.platform.is_empty() {
                "not recorded"
            } else {
                &data.environment.platform
            },
            if data.environment.os_version.is_empty() {
                "not recorded"
            } else {
                &data.environment.os_version
            },
            if data.environment.tokoro_version.is_empty() {
                "not recorded"
            } else {
                &data.environment.tokoro_version
            },
            data.environment.hardware,
            data.environment.unified_memory_gib,
            memory_kind,
            data.model.id,
            data.model.parameters,
            data.model.quantization,
            data.model.engine,
            if data.model.engine_version.is_empty() {
                "not reported"
            } else {
                &data.model.engine_version
            },
            data.model.mode
        ));
    }
    if recipe.sections.runs {
        output.push_str(
            "\n## Runs\n\n| run | prefill tok/s | decode tok/s | TTFT ms | TPOT ms | end-to-end ms | output tokens | token count |\n| ---: | ---: | ---: | ---: | ---: | ---: | ---: | :--- |\n",
        );
        for run in &data.runs {
            output.push_str(&format!(
                "| {} | {:.1} | {:.1} | {:.0} | {:.1} | {:.0} | {} | {} |\n",
                run.run,
                run.prefill_tokens_per_second,
                run.decode_tokens_per_second,
                run.ttft_milliseconds,
                run.time_per_output_token_milliseconds,
                run.end_to_end_milliseconds,
                run.output_tokens,
                if run.token_count_source.is_empty() {
                    "not recorded"
                } else {
                    &run.token_count_source
                }
            ));
        }
        if data.runs.is_empty() {
            output.push_str("| - | - | - | - | - | - | - | - |\n");
        }
        if !data.concurrency.is_empty() {
            output.push_str("\n## Concurrency sweep\n\n| concurrent | completed | errors | system tok/s | request tok/s | p95 latency ms | p95 TPOT ms | waiting peak | KV peak | RSS GiB | swap MiB |\n| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
            for point in &data.concurrency {
                output.push_str(&format!(
                    "| {} | {} | {} | {:.1} | {:.1} | {:.0} | {:.1} | {} | {} | {:.1} | {:.0} |\n",
                    point.concurrency,
                    point.completed,
                    point.errors,
                    point.system_tokens_per_second,
                    point.mean_request_tokens_per_second,
                    point.p95_latency_milliseconds,
                    point.p95_time_per_output_token_milliseconds,
                    point
                        .peak_waiting_requests
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "?".into()),
                    point
                        .peak_kv_cache_usage
                        .map(|value| format!("{value:.1}%"))
                        .unwrap_or_else(|| "?".into()),
                    point.peak_server_rss_gib,
                    point.peak_swap_mib
                ));
            }
        }
    }
    if recipe.sections.sweep && !data.sweep.is_empty() {
        output
            .push_str("\n## Prompt sweep\n\n| prompt tokens | prefill tok/s |\n| ---: | ---: |\n");
        for point in &data.sweep {
            output.push_str(&format!(
                "| {} | {:.1} |\n",
                point.prompt_tokens, point.prefill_tokens_per_second
            ));
        }
    }
    if recipe.sections.methodology {
        output.push_str("\n## Methodology\n\n");
        output.push_str(&format!(
            "Temperature {:.0}; {} prompt tokens; {} output-token limit; {} requested runs{}. Prompts and responses are excluded from the bundle.\n",
            data.workload.temperature,
            data.workload.prompt_tokens,
            data.workload.output_limit_tokens,
            data.workload.requested_runs,
            if data.workload.concurrency_levels.is_empty() {
                String::new()
            } else {
                format!("; concurrency {:?}", data.workload.concurrency_levels)
            }
        ));
        if !data.budgets.is_empty() {
            output.push_str("\nBudgets are user-defined for this workload. Tokoro does not apply vendor thresholds.\n");
            for budget in &data.budgets {
                output.push_str(&format!(
                    "- {} {} {:.1} {}: {}. {}\n",
                    budget.metric,
                    if budget.relation == "at_most" {
                        "<="
                    } else {
                        ">="
                    },
                    budget.target,
                    budget.unit,
                    budget.status,
                    budget.evidence
                ));
            }
        }
    }
    if recipe.sections.provenance {
        output.push_str("\n## Provenance\n\n");
        output.push_str(&format!(
            "- rates: {}\n- timing: {}\n- environment: {}\n- custody: {}\n",
            data.provenance.rates,
            data.provenance.timing,
            data.provenance.environment,
            data.provenance.custody
        ));
    }
    if recipe.sections.privacy {
        output.push_str("\n## Privacy receipt\n\n");
        output.push_str(&format!(
            "Excluded: {}.\n",
            data.privacy.excluded.join(", ")
        ));
    }
    if !recipe.narrative.conclusion.trim().is_empty() {
        output.push_str(&format!(
            "\n## Conclusion\n\n{}\n",
            clean_narrative(&recipe.narrative.conclusion)
        ));
    }
    Ok(output)
}

pub(crate) fn render_json(envelope: &ReportEnvelope) -> Result<String, String> {
    verify(envelope)?;
    serde_json::to_string_pretty(envelope).map_err(|error| error.to_string())
}

pub(crate) fn render_csv(envelope: &ReportEnvelope) -> Result<String, String> {
    verify(envelope)?;
    let data = &envelope.data;
    let mut rows = vec![vec![
        "schema".into(),
        "bundle_sha256".into(),
        "kind".into(),
        "run".into(),
        "concurrency".into(),
        "prompt_tokens".into(),
        "output_tokens".into(),
        "prefill_tokens_per_second".into(),
        "decode_tokens_per_second".into(),
        "ttft_milliseconds".into(),
        "time_per_output_token_milliseconds".into(),
        "end_to_end_milliseconds".into(),
        "system_tokens_per_second".into(),
        "p95_latency_milliseconds".into(),
        "p95_time_per_output_token_milliseconds".into(),
        "completed".into(),
        "errors".into(),
        "peak_waiting_requests".into(),
        "peak_kv_cache_percent".into(),
        "peak_server_rss_gib".into(),
        "peak_swap_mib".into(),
    ]];
    for run in &data.runs {
        rows.push(vec![
            REPORT_SCHEMA.into(),
            envelope.sha256.clone(),
            "run".into(),
            run.run.to_string(),
            String::new(),
            data.workload.prompt_tokens.to_string(),
            run.output_tokens.to_string(),
            format!("{:.1}", run.prefill_tokens_per_second),
            format!("{:.1}", run.decode_tokens_per_second),
            format!("{:.0}", run.ttft_milliseconds),
            format!("{:.1}", run.time_per_output_token_milliseconds),
            format!("{:.0}", run.end_to_end_milliseconds),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            format!("{:.1}", data.benchmark_pressure.peak_server_rss_gib),
            format!("{:.0}", data.benchmark_pressure.peak_swap_mib),
        ]);
    }
    for point in &data.sweep {
        let mut row = vec![String::new(); rows[0].len()];
        row[0] = REPORT_SCHEMA.into();
        row[1] = envelope.sha256.clone();
        row[2] = "sweep".into();
        row[5] = point.prompt_tokens.to_string();
        row[7] = format!("{:.1}", point.prefill_tokens_per_second);
        rows.push(row);
    }
    for point in &data.concurrency {
        let mut row = vec![String::new(); rows[0].len()];
        row[0] = REPORT_SCHEMA.into();
        row[1] = envelope.sha256.clone();
        row[2] = "concurrency".into();
        row[4] = point.concurrency.to_string();
        row[5] = data.workload.prompt_tokens.to_string();
        row[8] = format!("{:.1}", point.mean_request_tokens_per_second);
        row[12] = format!("{:.1}", point.system_tokens_per_second);
        row[13] = format!("{:.0}", point.p95_latency_milliseconds);
        row[14] = format!("{:.1}", point.p95_time_per_output_token_milliseconds);
        row[15] = point.completed.to_string();
        row[16] = point.errors.to_string();
        row[17] = point
            .peak_waiting_requests
            .map(|value| value.to_string())
            .unwrap_or_default();
        row[18] = point
            .peak_kv_cache_usage
            .map(|value| format!("{value:.1}"))
            .unwrap_or_default();
        row[19] = format!("{:.1}", point.peak_server_rss_gib);
        row[20] = format!("{:.0}", point.peak_swap_mib);
        rows.push(row);
    }
    Ok(rows
        .into_iter()
        .map(|row| row.join(","))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

pub(crate) fn render_prometheus(envelope: &ReportEnvelope) -> Result<String, String> {
    verify(envelope)?;
    let data = &envelope.data;
    let labels = format!(
        "model=\"{}\",engine=\"{}\",engine_version=\"{}\",workload=\"{}\",bundle=\"{}\"",
        prometheus_label(&data.model.id),
        prometheus_label(&data.model.engine),
        prometheus_label(if data.model.engine_version.is_empty() {
            "not_reported"
        } else {
            &data.model.engine_version
        }),
        prometheus_label(&data.workload.name),
        &envelope.sha256[..12]
    );
    let mut output = String::from(
        "# Tokoro checked benchmark handoff. No prompt or response content is included.\n",
    );
    let run_metrics = [
        (
            "tokoro_benchmark_prefill_tokens_per_second",
            "Prefill tokens per second for one checked run.",
        ),
        (
            "tokoro_benchmark_decode_tokens_per_second",
            "Per-request decode tokens per second for one checked run.",
        ),
        (
            "tokoro_benchmark_ttft_milliseconds",
            "Time to first token measured by Tokoro.",
        ),
        (
            "tokoro_benchmark_time_per_output_token_milliseconds",
            "Time per output token derived from measured decode duration.",
        ),
        (
            "tokoro_benchmark_end_to_end_milliseconds",
            "End-to-end request duration measured by Tokoro.",
        ),
    ];
    for (name, help) in run_metrics {
        output.push_str(&format!("# HELP {name} {help}\n# TYPE {name} gauge\n"));
    }
    for run in &data.runs {
        let run_labels = format!("{labels},run=\"{}\"", run.run);
        for (name, value) in [
            (
                "tokoro_benchmark_prefill_tokens_per_second",
                run.prefill_tokens_per_second,
            ),
            (
                "tokoro_benchmark_decode_tokens_per_second",
                run.decode_tokens_per_second,
            ),
            ("tokoro_benchmark_ttft_milliseconds", run.ttft_milliseconds),
            (
                "tokoro_benchmark_time_per_output_token_milliseconds",
                run.time_per_output_token_milliseconds,
            ),
            (
                "tokoro_benchmark_end_to_end_milliseconds",
                run.end_to_end_milliseconds,
            ),
        ] {
            output.push_str(&format!("{name}{{{run_labels}}} {value}\n"));
        }
    }
    let concurrency_metrics = [
        (
            "tokoro_benchmark_system_tokens_per_second",
            "Aggregate output tokens per wall second.",
        ),
        (
            "tokoro_benchmark_request_tokens_per_second",
            "Mean per-request decode tokens per second.",
        ),
        (
            "tokoro_benchmark_p95_latency_milliseconds",
            "P95 end-to-end latency within the concurrency point.",
        ),
        (
            "tokoro_benchmark_p95_time_per_output_token_milliseconds",
            "P95 time per output token within the concurrency point.",
        ),
        (
            "tokoro_benchmark_errors",
            "Failed requests within the concurrency point.",
        ),
    ];
    for (name, help) in concurrency_metrics {
        output.push_str(&format!("# HELP {name} {help}\n# TYPE {name} gauge\n"));
    }
    for point in &data.concurrency {
        let point_labels = format!("{labels},concurrency=\"{}\"", point.concurrency);
        for (name, value) in [
            (
                "tokoro_benchmark_system_tokens_per_second",
                point.system_tokens_per_second,
            ),
            (
                "tokoro_benchmark_request_tokens_per_second",
                point.mean_request_tokens_per_second,
            ),
            (
                "tokoro_benchmark_p95_latency_milliseconds",
                point.p95_latency_milliseconds,
            ),
            (
                "tokoro_benchmark_p95_time_per_output_token_milliseconds",
                point.p95_time_per_output_token_milliseconds,
            ),
            ("tokoro_benchmark_errors", point.errors as f64),
        ] {
            output.push_str(&format!("{name}{{{point_labels}}} {value}\n"));
        }
    }
    Ok(output)
}

pub(crate) fn render_otlp_json(envelope: &ReportEnvelope) -> Result<String, String> {
    verify(envelope)?;
    let data = &envelope.data;
    let timestamp = data.captured_unix.saturating_mul(1_000_000_000).to_string();
    let mut metrics = Vec::new();
    let mut add = |name: &str, unit: &str, value: f64, attributes: Vec<serde_json::Value>| {
        metrics.push(serde_json::json!({
            "name": name,
            "unit": unit,
            "gauge": {
                "dataPoints": [{
                    "attributes": attributes,
                    "timeUnixNano": timestamp,
                    "asDouble": value
                }]
            }
        }));
    };
    for run in &data.runs {
        let attributes = vec![otlp_attribute("run", run.run.to_string())];
        add(
            "tokoro.benchmark.prefill.tokens_per_second",
            "{token}/s",
            run.prefill_tokens_per_second,
            attributes.clone(),
        );
        add(
            "tokoro.benchmark.decode.tokens_per_second",
            "{token}/s",
            run.decode_tokens_per_second,
            attributes.clone(),
        );
        add(
            "tokoro.benchmark.ttft",
            "ms",
            run.ttft_milliseconds,
            attributes.clone(),
        );
        add(
            "tokoro.benchmark.time_per_output_token",
            "ms/{token}",
            run.time_per_output_token_milliseconds,
            attributes.clone(),
        );
        add(
            "tokoro.benchmark.end_to_end",
            "ms",
            run.end_to_end_milliseconds,
            attributes,
        );
    }
    for point in &data.concurrency {
        let attributes = vec![otlp_attribute("concurrency", point.concurrency.to_string())];
        add(
            "tokoro.benchmark.system.tokens_per_second",
            "{token}/s",
            point.system_tokens_per_second,
            attributes.clone(),
        );
        add(
            "tokoro.benchmark.request.p95_latency",
            "ms",
            point.p95_latency_milliseconds,
            attributes.clone(),
        );
        add(
            "tokoro.benchmark.request.p95_time_per_output_token",
            "ms/{token}",
            point.p95_time_per_output_token_milliseconds,
            attributes.clone(),
        );
        add(
            "tokoro.benchmark.errors",
            "{request}",
            point.errors as f64,
            attributes,
        );
    }
    let payload = serde_json::json!({
        "resourceMetrics": [{
            "resource": {
                "attributes": [
                    otlp_attribute("service.name", "tokoro"),
                    otlp_attribute("tokoro.report.schema", REPORT_SCHEMA),
                    otlp_attribute("tokoro.bundle.sha256", envelope.sha256.clone()),
                    otlp_attribute("model.id", data.model.id.clone()),
                    otlp_attribute("inference.engine", data.model.engine.clone()),
                    otlp_attribute(
                        "inference.engine.version",
                        if data.model.engine_version.is_empty() {
                            "not_reported".into()
                        } else {
                            data.model.engine_version.clone()
                        }
                    ),
                    otlp_attribute("os.version", data.environment.os_version.clone()),
                    otlp_attribute("benchmark.workload", data.workload.name.clone())
                ]
            },
            "scopeMetrics": [{
                "scope": {"name": "tokoro.report", "version": env!("CARGO_PKG_VERSION")},
                "metrics": metrics
            }]
        }]
    });
    serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct HistoryEntry {
    pub id: String,
    pub captured_unix: u64,
    pub model: String,
    pub engine: String,
    pub engine_version: String,
    pub workload: String,
    pub environment_id: String,
    pub workload_id: String,
    pub runs: usize,
    pub concurrency_points: usize,
    pub budget_breaches: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct HistoryIndex {
    pub entries: Vec<HistoryEntry>,
    pub rejected: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ComparisonDelta {
    pub metric: String,
    pub baseline: f64,
    pub candidate: f64,
    pub absolute: f64,
    pub percent: Option<f64>,
    pub unit: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReportComparison {
    pub baseline: String,
    pub candidate: String,
    pub comparable: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub configuration_changes: Vec<String>,
    pub deltas: Vec<ComparisonDelta>,
}

pub(crate) fn saved_history() -> Result<HistoryIndex, String> {
    let root = reports_root();
    let mut entries = Vec::new();
    let mut rejected = Vec::new();
    if !root.exists() {
        return Ok(HistoryIndex { entries, rejected });
    }
    for entry in fs::read_dir(&root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry.path().is_dir() {
            continue;
        }
        let bundle_path = entry.path().join("bundle.json");
        let id = entry.file_name().to_string_lossy().into_owned();
        match load_envelope(&bundle_path).and_then(|envelope| {
            verify(&envelope)?;
            if !has_measurements(&envelope.data) {
                return Err("no benchmark measurements".into());
            }
            Ok(history_entry(&envelope))
        }) {
            Ok(item) => entries.push(item),
            Err(error) => rejected.push(format!("{id}: {error}")),
        }
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.captured_unix));
    rejected.sort();
    Ok(HistoryIndex { entries, rejected })
}

pub(crate) fn load_measured(reference: &str) -> Result<ReportEnvelope, String> {
    let envelope = load_bundle_reference(reference)?;
    verify(&envelope)?;
    if !has_measurements(&envelope.data) {
        return Err("the checked report contains no benchmark measurements".into());
    }
    Ok(envelope)
}

pub(crate) fn compare_saved(baseline: &str, candidate: &str) -> Result<ReportComparison, String> {
    let baseline = load_measured(baseline)?;
    let candidate = load_measured(candidate)?;
    let mut blockers = Vec::new();
    if environment_identity(&baseline.data) != environment_identity(&candidate.data) {
        blockers.push(
            "environment identity differs: platform, hardware, memory kind, or capacity".into(),
        );
    }
    if workload_identity(&baseline.data) != workload_identity(&candidate.data) {
        blockers.push(
            "workload identity differs: recipe, token shape, temperature, runs, or concurrency"
                .into(),
        );
    }
    let mut warnings = Vec::new();
    if baseline.data.model.engine_version.is_empty()
        || candidate.data.model.engine_version.is_empty()
    {
        warnings.push(
            "runtime version was not reported for both runs; deltas may include an engine update"
                .into(),
        );
    }
    let mut configuration_changes = Vec::new();
    let baseline_model = public_model_id(&baseline.data.model.id);
    let candidate_model = public_model_id(&candidate.data.model.id);
    if baseline_model != candidate_model {
        configuration_changes.push(format!("model: {baseline_model} -> {candidate_model}"));
    }
    for (label, left, right) in [
        (
            "quantization",
            &baseline.data.model.quantization,
            &candidate.data.model.quantization,
        ),
        (
            "engine",
            &baseline.data.model.engine,
            &candidate.data.model.engine,
        ),
        (
            "engine version",
            &baseline.data.model.engine_version,
            &candidate.data.model.engine_version,
        ),
        (
            "mode",
            &baseline.data.model.mode,
            &candidate.data.model.mode,
        ),
    ] {
        if left != right {
            configuration_changes.push(format!("{label}: {left} -> {right}"));
        }
    }
    if baseline.data.model.context_limit_tokens != candidate.data.model.context_limit_tokens {
        configuration_changes.push(format!(
            "context limit: {} -> {} tokens",
            baseline.data.model.context_limit_tokens, candidate.data.model.context_limit_tokens
        ));
    }

    let mut deltas = Vec::new();
    if blockers.is_empty() {
        let baseline_means = means(&baseline.data.runs);
        let candidate_means = means(&candidate.data.runs);
        if let (Some(left), Some(right)) = (baseline_means, candidate_means) {
            add_delta(&mut deltas, "mean_prefill", left.0, right.0, "tok/s");
            add_delta(&mut deltas, "mean_decode", left.1, right.1, "tok/s");
            add_delta(&mut deltas, "mean_ttft", left.2, right.2, "ms");
        }
        if !baseline.data.runs.is_empty() && !candidate.data.runs.is_empty() {
            add_delta(
                &mut deltas,
                "mean_tpot",
                baseline
                    .data
                    .runs
                    .iter()
                    .map(|run| run.time_per_output_token_milliseconds)
                    .sum::<f64>()
                    / baseline.data.runs.len() as f64,
                candidate
                    .data
                    .runs
                    .iter()
                    .map(|run| run.time_per_output_token_milliseconds)
                    .sum::<f64>()
                    / candidate.data.runs.len() as f64,
                "ms/token",
            );
        }
        if let (Some(left), Some(right)) = (
            best_system_throughput(&baseline.data.concurrency),
            best_system_throughput(&candidate.data.concurrency),
        ) {
            add_delta(&mut deltas, "best_system_throughput", left, right, "tok/s");
        }
        if let (Some(left), Some(right)) = (
            highest_concurrency_p95(&baseline.data.concurrency),
            highest_concurrency_p95(&candidate.data.concurrency),
        ) {
            add_delta(&mut deltas, "highest_concurrency_p95", left, right, "ms");
        }
        if !baseline.data.benchmark_pressure.is_empty()
            && !candidate.data.benchmark_pressure.is_empty()
        {
            add_delta(
                &mut deltas,
                "peak_server_rss",
                baseline.data.benchmark_pressure.peak_server_rss_gib,
                candidate.data.benchmark_pressure.peak_server_rss_gib,
                "GiB",
            );
            add_delta(
                &mut deltas,
                "peak_swap",
                baseline.data.benchmark_pressure.peak_swap_mib,
                candidate.data.benchmark_pressure.peak_swap_mib,
                "MiB",
            );
        }
    }

    Ok(ReportComparison {
        baseline: baseline.sha256[..12].into(),
        candidate: candidate.sha256[..12].into(),
        comparable: blockers.is_empty(),
        blockers,
        warnings,
        configuration_changes,
        deltas,
    })
}

fn reports_root() -> PathBuf {
    platform::state_home().join("tokoro").join("reports")
}

fn load_bundle_reference(reference: &str) -> Result<ReportEnvelope, String> {
    let path = PathBuf::from(reference);
    let path = if path.is_file() {
        path
    } else if path.is_dir() {
        path.join("bundle.json")
    } else {
        reports_root().join(reference).join("bundle.json")
    };
    load_envelope(&path)
}

fn load_envelope(path: &Path) -> Result<ReportEnvelope, String> {
    let content =
        fs::read_to_string(path).map_err(|error| format!("could not read bundle: {error}"))?;
    serde_json::from_str::<ReportEnvelope>(&content)
        .map_err(|error| format!("invalid bundle JSON: {error}"))
}

fn has_measurements(data: &ReportData) -> bool {
    !data.runs.is_empty() || !data.sweep.is_empty() || !data.concurrency.is_empty()
}

fn history_entry(envelope: &ReportEnvelope) -> HistoryEntry {
    HistoryEntry {
        id: envelope.sha256[..12].into(),
        captured_unix: envelope.data.captured_unix,
        model: public_model_id(&envelope.data.model.id),
        engine: envelope.data.model.engine.clone(),
        engine_version: if envelope.data.model.engine_version.is_empty() {
            "not reported".into()
        } else {
            envelope.data.model.engine_version.clone()
        },
        workload: envelope.data.workload.name.clone(),
        environment_id: short_hash(&environment_identity(&envelope.data)),
        workload_id: short_hash(&workload_identity(&envelope.data)),
        runs: envelope.data.runs.len(),
        concurrency_points: envelope.data.concurrency.len(),
        budget_breaches: envelope
            .data
            .budgets
            .iter()
            .filter(|budget| budget.status == "breach")
            .count(),
    }
}

fn environment_identity(data: &ReportData) -> String {
    format!(
        "{}|{}|{}|{}|{}|{:.1}",
        data.environment.platform,
        data.environment.os_version,
        data.environment.tokoro_version,
        data.environment.hardware,
        data.environment.memory_kind,
        data.environment.unified_memory_gib
    )
}

fn workload_identity(data: &ReportData) -> String {
    format!(
        "{}|{}|{}|{}|{:.3}|{:?}",
        data.workload.name,
        data.workload.prompt_tokens,
        data.workload.output_limit_tokens,
        data.workload.requested_runs,
        data.workload.temperature,
        data.workload.concurrency_levels
    )
}

fn short_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))[..12].into()
}

fn add_delta(
    deltas: &mut Vec<ComparisonDelta>,
    metric: &str,
    baseline: f64,
    candidate: f64,
    unit: &str,
) {
    if !baseline.is_finite() || !candidate.is_finite() {
        return;
    }
    deltas.push(ComparisonDelta {
        metric: metric.into(),
        baseline: round_one(baseline),
        candidate: round_one(candidate),
        absolute: round_one(candidate - baseline),
        percent: (baseline.abs() > f64::EPSILON)
            .then_some(round_one((candidate - baseline) / baseline * 100.0)),
        unit: unit.into(),
    });
}

fn best_system_throughput(points: &[ConcurrencyReading]) -> Option<f64> {
    points
        .iter()
        .filter(|point| point.errors == 0)
        .map(|point| point.system_tokens_per_second)
        .max_by(f64::total_cmp)
}

fn highest_concurrency_p95(points: &[ConcurrencyReading]) -> Option<f64> {
    points
        .iter()
        .filter(|point| point.errors == 0)
        .max_by_key(|point| point.concurrency)
        .map(|point| point.p95_latency_milliseconds)
}

fn prometheus_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn otlp_attribute(key: &str, value: impl Into<String>) -> serde_json::Value {
    serde_json::json!({"key": key, "value": {"stringValue": value.into()}})
}

pub(crate) fn benchmark_markdown(app: &App) -> String {
    capture(app)
        .and_then(|bundle| render_markdown(&bundle, &ReportRecipe::default()))
        .unwrap_or_else(|error| format!("# Tokoro report unavailable\n\n{error}\n"))
}

pub(crate) fn save_private_report(app: &App) -> Result<String, String> {
    let envelope = capture(app)?;
    if !has_measurements(&envelope.data) {
        return Err("run a benchmark before saving a checked report pack".into());
    }
    let report_id = envelope.sha256[..12].to_string();
    let directory = reports_root().join(&report_id);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let bundle_path = directory.join("bundle.json");
    let recipe_path = directory.join("report.toml");
    fs::write(&bundle_path, render_json(&envelope)?).map_err(|error| error.to_string())?;
    if !recipe_path.exists() {
        fs::write(&recipe_path, recipe_toml(&ReportRecipe::default())?)
            .map_err(|error| error.to_string())?;
    }
    let recipe =
        parse_recipe(&fs::read_to_string(&recipe_path).map_err(|error| error.to_string())?)?;
    fs::write(
        directory.join("report.md"),
        render_markdown(&envelope, &recipe)?,
    )
    .map_err(|error| error.to_string())?;
    fs::write(directory.join("runs.csv"), render_csv(&envelope)?)
        .map_err(|error| error.to_string())?;
    Ok(report_id)
}

pub(crate) fn write_default_recipe(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, recipe_toml(&ReportRecipe::default())?).map_err(|error| error.to_string())
}

pub(crate) fn render_saved(
    bundle_path: &Path,
    recipe_path: Option<&Path>,
    format: &str,
) -> Result<String, String> {
    let envelope = serde_json::from_str::<ReportEnvelope>(
        &fs::read_to_string(bundle_path).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    match format {
        "json" => render_json(&envelope),
        "csv" => render_csv(&envelope),
        "prometheus" | "prom" => render_prometheus(&envelope),
        "otlp-json" | "otlp" => render_otlp_json(&envelope),
        "markdown" | "md" => {
            let recipe = if let Some(path) = recipe_path {
                parse_recipe(&fs::read_to_string(path).map_err(|error| error.to_string())?)?
            } else {
                ReportRecipe::default()
            };
            render_markdown(&envelope, &recipe)
        }
        _ => Err("report format must be markdown, json, csv, prometheus, or otlp-json".into()),
    }
}

fn means(runs: &[RunReading]) -> Option<(f64, f64, f64)> {
    if runs.is_empty() {
        return None;
    }
    let count = runs.len() as f64;
    Some((
        runs.iter()
            .map(|run| run.prefill_tokens_per_second)
            .sum::<f64>()
            / count,
        runs.iter()
            .map(|run| run.decode_tokens_per_second)
            .sum::<f64>()
            / count,
        runs.iter().map(|run| run.ttft_milliseconds).sum::<f64>() / count,
    ))
}

fn clean_narrative(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn percentile(values: impl Iterator<Item = f64>, quantile: f64) -> Option<f64> {
    let mut values = values.filter(|value| value.is_finite()).collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    let index = ((values.len() - 1) as f64 * quantile.clamp(0.0, 1.0)).ceil() as usize;
    values.get(index).copied()
}

fn is_zero_f64(value: &f64) -> bool {
    *value == 0.0
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

fn round_one(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    fn app() -> App {
        let mut config = Config::default();
        config.bloat.scan_project = false;
        let mut app = App::new(config);
        app.chip = "Test Chip".into();
        app.total_mem_gb = 64.0;
        app.online = true;
        app.model = "/private/models/test-model".into();
        app.engine = "test-runtime".into();
        app.bench.started_unix = 1_700_000_000;
        app.bench.label = "quick response".into();
        app.bench.prompt_tokens = 512;
        app.bench.gen_tokens = 64;
        app.bench.runs = 2;
        app.bench.results = vec![
            crate::BenchRun {
                pp: 100.0,
                tg: 20.0,
                ttft_ms: 80.0,
                tpot_ms: 47.3,
                end_to_end_ms: 600.0,
                output_tokens: 12,
                token_count_source: "server-reported usage".into(),
            },
            crate::BenchRun {
                pp: 120.0,
                tg: 22.0,
                ttft_ms: 70.0,
                tpot_ms: 43.6,
                end_to_end_ms: 550.0,
                output_tokens: 12,
                token_count_source: "server-reported usage".into(),
            },
        ];
        app
    }

    #[test]
    fn report_rendering_is_deterministic_for_a_bundle_and_recipe() {
        let envelope = capture(&app()).expect("capture");
        let recipe = ReportRecipe::default();
        assert_eq!(
            render_markdown(&envelope, &recipe).expect("first render"),
            render_markdown(&envelope, &recipe).expect("second render")
        );
        assert_eq!(
            render_json(&envelope).expect("first JSON"),
            render_json(&envelope).expect("second JSON")
        );
    }

    #[test]
    fn recipe_changes_presentation_without_changing_measurements() {
        let envelope = capture(&app()).expect("capture");
        let original_hash = envelope.sha256.clone();
        let mut sections = ReportRecipe::default().sections;
        sections.environment = false;
        let recipe = ReportRecipe {
            title: "My local result".into(),
            sections,
            narrative: ReportNarrative {
                conclusion: "Use the measured baseline.".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let markdown = render_markdown(&envelope, &recipe).expect("render");
        assert!(markdown.contains("My local result"));
        assert!(markdown.contains("20.0"));
        assert!(!markdown.contains("## Environment"));
        assert_eq!(envelope.sha256, original_hash);
    }

    #[test]
    fn changed_measurements_fail_bundle_verification() {
        let mut envelope = capture(&app()).expect("capture");
        envelope.data.runs[0].decode_tokens_per_second = 999.0;
        assert!(verify(&envelope).is_err());
    }

    #[test]
    fn public_bundle_excludes_local_paths_and_prompt_content() {
        let envelope = capture(&app()).expect("capture");
        let json = render_json(&envelope).expect("JSON");
        assert!(!json.contains("/private/models"));
        assert!(!json.contains("Count rapidly"));
        assert!(json.contains("test-model"));
        assert!(!envelope.data.privacy.prompts_included);
    }

    #[test]
    fn concurrency_pressure_and_token_provenance_are_immutable_measurements() {
        let mut app = app();
        app.bench.concurrency_levels = vec![1, 2, 4, 8];
        app.bench.concurrency_results.push(crate::ConcurrencyRun {
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
        let envelope = capture(&app).expect("capture");
        let point = &envelope.data.concurrency[0];
        assert_eq!(point.concurrency, 4);
        assert_eq!(point.peak_waiting_requests, Some(2));
        assert_eq!(point.peak_kv_cache_usage, Some(74.0));
        assert_eq!(point.token_count_source, "server-reported usage");
        verify(&envelope).expect("checked concurrency bundle");
    }

    #[test]
    fn workload_budgets_use_only_user_configured_thresholds() {
        let mut app = app();
        app.bench.runs = 3;
        app.bench.results.push(crate::BenchRun {
            pp: 110.0,
            tg: 21.0,
            ttft_ms: 90.0,
            tpot_ms: 44.5,
            end_to_end_ms: 580.0,
            output_tokens: 12,
            token_count_source: "server-reported usage".into(),
        });
        app.cfg
            .benchmark
            .budgets
            .push(crate::settings::WorkloadBudget {
                workload: "quick response".into(),
                max_ttft_p95_ms: Some(85.0),
                max_tpot_p95_ms: Some(45.0),
                min_decode_tokens_per_second: Some(20.0),
                ..Default::default()
            });
        let assessments = budget_assessments(&app);
        assert_eq!(assessments.len(), 3);
        assert_eq!(assessments[0].metric, "ttft_p95");
        assert_eq!(assessments[0].status, "breach");
        assert_eq!(assessments[1].metric, "tpot_p95");
        assert_eq!(assessments[1].status, "breach");
        assert_eq!(assessments[2].status, "pass");
    }

    #[test]
    fn checked_exporters_emit_metrics_without_content() {
        let envelope = capture(&app()).expect("capture");
        let prometheus = render_prometheus(&envelope).expect("prometheus");
        assert_eq!(
            prometheus
                .matches("# HELP tokoro_benchmark_decode_tokens_per_second")
                .count(),
            1
        );
        assert!(prometheus.contains("bundle=\""));
        assert!(!prometheus.contains("Count rapidly"));

        let otlp = render_otlp_json(&envelope).expect("OTLP JSON");
        let parsed: serde_json::Value = serde_json::from_str(&otlp).expect("valid JSON");
        assert!(parsed["resourceMetrics"][0]["scopeMetrics"][0]["metrics"].is_array());
        assert!(!otlp.contains("/private/models"));
    }

    #[test]
    fn comparison_blocks_unlike_workloads_and_diffs_like_runs() {
        let baseline = capture(&app()).expect("baseline");
        let mut candidate_data = baseline.data.clone();
        candidate_data.model.id = "candidate-model".into();
        candidate_data.runs[0].decode_tokens_per_second = 24.0;
        candidate_data.runs[1].decode_tokens_per_second = 26.0;
        let candidate = envelope(candidate_data).expect("candidate");
        let root =
            std::env::temp_dir().join(format!("tokoro-report-comparison-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp directory");
        let baseline_path = root.join("baseline.json");
        let candidate_path = root.join("candidate.json");
        fs::write(
            &baseline_path,
            render_json(&baseline).expect("baseline JSON"),
        )
        .expect("write baseline");
        fs::write(
            &candidate_path,
            render_json(&candidate).expect("candidate JSON"),
        )
        .expect("write candidate");

        let comparison = compare_saved(
            baseline_path.to_str().expect("baseline path"),
            candidate_path.to_str().expect("candidate path"),
        )
        .expect("comparison");
        assert!(comparison.comparable);
        assert_eq!(comparison.warnings.len(), 1);
        assert!(comparison
            .configuration_changes
            .iter()
            .any(|change| change.starts_with("model:")));
        assert!(comparison
            .deltas
            .iter()
            .any(|delta| delta.metric == "mean_decode" && delta.absolute > 0.0));

        let mut unlike_data = candidate.data.clone();
        unlike_data.workload.prompt_tokens = 2048;
        let unlike = envelope(unlike_data).expect("unlike");
        fs::write(&candidate_path, render_json(&unlike).expect("unlike JSON"))
            .expect("write unlike");
        let blocked = compare_saved(
            baseline_path.to_str().expect("baseline path"),
            candidate_path.to_str().expect("candidate path"),
        )
        .expect("blocked comparison");
        assert!(!blocked.comparable);
        assert!(blocked.deltas.is_empty());
        assert!(!blocked.blockers.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn csv_rows_keep_the_declared_column_count() {
        let csv = render_csv(&capture(&app()).expect("capture")).expect("CSV");
        let mut lines = csv.lines();
        let columns = lines.next().expect("header").split(',').count();
        assert!(lines.all(|line| line.split(',').count() == columns));
    }
}
